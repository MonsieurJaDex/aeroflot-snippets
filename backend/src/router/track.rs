//! `/api/v1/track` — the organiser contract for engineer, vehicle and aircraft
//! points. Accepts a JSON body and the query string a GPSLogger *Custom URL*
//! callback sends, both reduced to the same [`TrackSample`].

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{FromRequest, Query, Request, State};
use axum::http::{header, Response, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, SecondsFormat, Utc};
use redis::TypedCommands;
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::{active_map, bad_request};
use crate::graph::AgentKind;
use crate::location::ExternalAgent;
use crate::types::map::Point;
use crate::types::config::AppState;
use crate::types::enums::VehicleType;

#[derive(Debug, Deserialize, ToSchema)]
pub struct TrackPoint {
    pub obj: String,
    /// Required latitude in degrees.
    pub lat: f64,
    /// Required longitude in degrees.
    pub lon: f64,
    pub t: Option<String>,
    pub acc: Option<f64>,
    pub spd: Option<f64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TrackBatch {
    /// contract: a malformed element is dropped instead of failing the batch, so
    /// one bad point does not reject the points around it.
    #[serde(deserialize_with = "lenient_points")]
    pub points: Vec<TrackPoint>,
}

/// Drops the elements of `points` that do not parse into a [`TrackPoint`].
fn lenient_points<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<TrackPoint>, D::Error> {
    Ok(Vec::<serde_json::Value>::deserialize(deserializer)?
        .into_iter()
        .filter_map(|point| match serde_json::from_value(point) {
            Ok(point) => Some(point),
            Err(error) => {
                tracing::warn!(%error, "skipping a track point that does not match the contract");
                None
            }
        })
        .collect())
}

/// Whether the request carried one point or `points`, which decides the shape of
/// the answer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Single,
    Batch,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TrackAck {
    pub accepted: bool,
    pub applied: bool,
    pub obj: String,
    pub t: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TrackBody {
    Many(TrackBatch),
    One(TrackPoint),
}

/// Body guard of the POST endpoint: it owns the media-type check and the error
/// mapping of a body that does not parse.
pub struct TrackJson(TrackBody);

impl<S: Send + Sync> FromRequest<S> for TrackJson {
    type Rejection = Response<Body>;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // contract: a missing Content-Type or one that is not JSON is 415, and the
        // handler never runs; parameters such as `charset=utf-8` are allowed.
        if !json_content_type(req.headers().get(header::CONTENT_TYPE)) {
            return Err(super::api_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "UNSUPPORTED_MEDIA_TYPE",
                "Content-Type must be application/json",
            ));
        }
        // contract: an unparseable body is 400, not axum's own 415/422, so the
        // client sees the same shape as a business-rule rejection.
        let Json(body) = Json::<TrackBody>::from_request(req, state)
            .await
            .map_err(|rejection| bad_request(rejection.body_text()))?;
        Ok(TrackJson(body))
    }
}

/// Accepts `application/json` and any `+json` media type, ignoring parameters.
fn json_content_type(raw: Option<&axum::http::HeaderValue>) -> bool {
    raw.and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or_default().trim())
        .is_some_and(|essence| {
            essence.eq_ignore_ascii_case("application/json")
                || essence.to_ascii_lowercase().ends_with("+json")
        })
}

/// Query of a GPSLogger *Custom URL* callback.
///
/// GPSLogger substitutes the `%name` placeholders of the configured URL and
/// sends the result as a `GET`, so every value arrives as a query parameter.
/// Placeholder names are substituted case-insensitively, hence the lenient
/// [`Deserialize`] below, and a value the device could not measure arrives as an
/// empty string or `NaN`.
#[derive(Debug, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TrackQuery {
    /// Object the point belongs to: a map agent, an engineer UUID or an
    /// aircraft registration. A fixed part of the configured URL.
    pub obj: Option<String>,
    /// `%aid` — Android id, used as the object when `obj` is absent.
    pub aid: Option<String>,
    /// `%ser` — device serial, used as the object when `obj` and `aid` are absent.
    pub ser: Option<String>,
    /// `%lat` — latitude in degrees.
    pub lat: Option<String>,
    /// `%lon` — longitude in degrees.
    pub lon: Option<String>,
    /// `%acc` — horizontal accuracy in metres.
    pub acc: Option<String>,
    /// `%spd` — speed in metres per second.
    pub spd: Option<String>,
    /// `%timestamp` — acquisition time in unix seconds. Preferred time source.
    pub timestamp: Option<String>,
    /// `%time` — acquisition time as UTC RFC 3339.
    pub time: Option<String>,
    /// `%timeoffset` — acquisition time with the device timezone offset.
    pub timeoffset: Option<String>,
}

impl<'de> Deserialize<'de> for TrackQuery {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut query = TrackQuery::default();
        for (name, value) in HashMap::<String, String>::deserialize(deserializer)? {
            match name.to_ascii_lowercase().as_str() {
                "obj" => query.obj = Some(value),
                "aid" => query.aid = Some(value),
                "ser" => query.ser = Some(value),
                "lat" => query.lat = Some(value),
                "lon" => query.lon = Some(value),
                "acc" => query.acc = Some(value),
                "spd" => query.spd = Some(value),
                "timestamp" => query.timestamp = Some(value),
                "time" => query.time = Some(value),
                "timeoffset" => query.timeoffset = Some(value),
                _ => {}
            }
        }
        Ok(query)
    }
}

/// One resolved position report, whichever transport carried it.
struct TrackSample {
    obj: String,
    lat: Option<f64>,
    lon: Option<f64>,
    when: Option<DateTime<Utc>>,
    acc: Option<f64>,
    spd: Option<f64>,
}

impl TrackPoint {
    fn into_sample(self) -> TrackSample {
        TrackSample {
            obj: self.obj,
            // contract: the JSON body requires both coordinates, so they are always
            // present here. `TrackSample` itself stays optional because
            // `TrackQuery::into_sample` yields `None` for a missing or `NaN` value.
            lat: Some(self.lat),
            lon: Some(self.lon),
            when: rfc3339(self.t.as_deref()),
            acc: self.acc,
            spd: self.spd,
        }
    }
}

impl TrackQuery {
    fn into_sample(self) -> TrackSample {
        let when = epoch_seconds(self.timestamp.as_deref())
            .or_else(|| rfc3339(self.time.as_deref()))
            .or_else(|| rfc3339(self.timeoffset.as_deref()));
        TrackSample {
            obj: non_empty(self.obj)
                .or_else(|| non_empty(self.aid))
                .or_else(|| non_empty(self.ser))
                .unwrap_or_default(),
            lat: number(self.lat.as_deref()),
            lon: number(self.lon.as_deref()),
            when,
            acc: number(self.acc.as_deref()),
            spd: number(self.spd.as_deref()),
        }
    }
}

/// Parses a numeric query value, treating the empty string and `NaN` — what
/// GPSLogger sends for an unknown measurement — as absent.
fn number(raw: Option<&str>) -> Option<f64> {
    let value: f64 = raw?.trim().parse().ok()?;
    value.is_finite().then_some(value)
}

fn non_empty(raw: Option<String>) -> Option<String> {
    raw.filter(|value| !value.trim().is_empty())
}

fn rfc3339(raw: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw?.trim())
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

/// Parses `%timestamp` — unix seconds, possibly fractional.
fn epoch_seconds(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let seconds = number(raw)?;
    DateTime::from_timestamp_millis((seconds * 1000.0) as i64)
}

#[utoipa::path(
    post,
    path = "/api/v1/track",
    tag = "Geo",
    operation_id = "ingestTrack",
    request_body = TrackPoint,
    responses(
        (status = 200, description = "Point or batch accepted", body = TrackAck),
        (status = 400, description = "Malformed JSON, missing obj or non-numeric lat/lon"),
        (status = 415, description = "Content-Type is not application/json"),
        (status = 500, description = "Internal storage failure"),
    ),
)]
pub async fn ingest(
    State(app_state): State<Arc<AppState>>,
    TrackJson(body): TrackJson,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    dbg!(&body);
    // contract: one point answers with a single object, `{ "points": [...] }` with
    // an array in request order — even when the array holds a single point.
    let (samples, shape) = match body {
        TrackBody::Many(batch) => (
            batch
                .points
                .into_iter()
                .map(TrackPoint::into_sample)
                .collect(),
            Shape::Batch,
        ),
        TrackBody::One(point) => (vec![point.into_sample()], Shape::Single),
    };
    ingest_samples(&app_state, samples, shape).await
}

/// Accepts one point of a GPSLogger *Custom URL* callback.
///
/// Point the app at
/// `https://<host>/api/v1/track?obj=<engineer>&lat=%LAT&lon=%LON&acc=%ACC&spd=%SPD&timestamp=%TIMESTAMP`
/// and it becomes a position source for the map. The app queues the requests
/// while offline and repeats each logged line, so a sample older than the one
/// already stored is acknowledged and ignored.
#[utoipa::path(
    get,
    path = "/api/v1/track",
    tag = "Geo",
    operation_id = "ingestTrackCustomUrl",
    params(TrackQuery),
    responses(
        (status = 200, description = "Point accepted", body = TrackAck),
        (status = 400, description = "lat/lon missing or not numeric"),
    ),
)]
pub async fn ingest_query(
    State(app_state): State<Arc<AppState>>,
    Query(query): Query<TrackQuery>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    ingest_samples(&app_state, vec![query.into_sample()], Shape::Single).await
}

/// Validates, applies and acknowledges the samples of one request.
async fn ingest_samples(
    app_state: &AppState,
    samples: Vec<TrackSample>,
    shape: Shape,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    // contract: a batch already dropped its malformed points while parsing, so a
    // request reaching storage is either valid or a single point that has to be
    // rejected as a whole. Checking before the first write keeps a rejected
    // request out of Redis and the database.
    if let Some(reason) = samples.iter().find_map(invalid_reason) {
        return Err(bad_request(reason));
    }
    let active = active_map(app_state)?;
    let mut redis_conn = app_state
        .redis_pool
        .get()
        .map_err(|_| super::internal_error())?;
    let mut pg_conn = app_state
        .db_pool
        .get()
        .map_err(|_| super::internal_error())?;
    let mut acks = Vec::with_capacity(samples.len());
    for sample in samples {
        acks.push(apply_point(&active, &mut redis_conn, &mut pg_conn, sample)?);
    }
    if shape == Shape::Single {
        Ok(Json(acks.pop().expect("one ack")).into_response())
    } else {
        Ok(Json(acks).into_response())
    }
}

/// Why a sample cannot be placed, or `None` when it can.
fn invalid_reason(sample: &TrackSample) -> Option<&'static str> {
    if sample.obj.trim().is_empty() {
        Some("obj is required")
    } else if sample.lat.is_none() || sample.lon.is_none() {
        Some("lat and lon are required numbers")
    } else {
        None
    }
}

/// How many points one object's track keeps, newest first.
///
/// Every accepted point is appended, so a dispatcher watching several aircraft,
/// vehicles and engineers moving at the same time reads each one's history
/// instead of only its last sample. The bound keeps a long-running exercise from
/// growing without limit; at a one-second logging interval it is under an hour and
/// a half, at the usual GPSLogger minute it is most of a week.
const TRACK_HISTORY_POINTS: i32 = 5000;

/// The object a point belongs to, resolved once per request.
///
/// A point may address a map agent, an engineer UUID or an aircraft registration;
/// whichever it is, everything below needs the same three answers: what kind of
/// thing moves, which UUID the position keys use, and whether the object exists at
/// all.
struct TrackedObject<'a> {
    agent: Option<&'a ExternalAgent>,
    /// UUID the position keys are written under, when the object resolves to one.
    linked: Option<Uuid>,
    kind: &'static str,
}

impl TrackedObject<'_> {
    fn agent_kind(&self) -> AgentKind {
        if self.kind == "vehicle" {
            AgentKind::Vehicle
        } else {
            AgentKind::Engineer
        }
    }

    /// UUID the shared position keys use: the linked engineer, else a stable v5
    /// name derived from `obj`, so an object without a row still has one key.
    fn position_id(&self, obj: &str) -> Uuid {
        self.linked
            .unwrap_or_else(|| Uuid::new_v5(&Uuid::NAMESPACE_OID, obj.as_bytes()))
    }
}

impl TrackSample {
    /// Resolves the object this point belongs to, or `None` when it names nothing
    /// the map, the database or a UUID in the payload knows.
    fn resolve<'a>(
        &self,
        active: &'a crate::maps::ActiveMap,
        pg_conn: &mut diesel::PgConnection,
    ) -> Option<TrackedObject<'a>> {
        let agent = active.location.agents.iter().find(|item| item.obj == self.obj);
        let linked = agent
            .and_then(|item| item.engineer_id.as_deref())
            .and_then(|id| Uuid::parse_str(id).ok())
            .or_else(|| Uuid::parse_str(&self.obj).ok());
        let known = agent.is_some() || linked.is_some_and(|id| engineer_exists(pg_conn, id));
        if !known {
            return None;
        }
        // The map knows what an object is; a registration or a `v-` name says it
        // itself when the map does not list it.
        let kind = agent.map(|item| item.kind.as_str()).unwrap_or({
            if self.obj.starts_with("v-") {
                "vehicle"
            } else if self.obj.starts_with("RA-") {
                "aircraft"
            } else {
                "engineer"
            }
        });
        let kind = match kind {
            "vehicle" => "vehicle",
            "aircraft" => "aircraft",
            _ => "engineer",
        };
        Some(TrackedObject {
            agent,
            linked,
            kind,
        })
    }

    /// The RFC 3339 stamp this point is stored and acknowledged with.
    fn stamp(&self) -> String {
        self.when
            .unwrap_or_else(Utc::now)
            .to_rfc3339_opts(SecondsFormat::Millis, true)
    }
}

/// Key holding one object's track, newest point first.
fn track_key(active: &crate::maps::ActiveMap, obj: &str) -> String {
    format!("map:{}:track:{}", active.id, obj)
}

/// The newest stored point of an object, verbatim.
fn previous_point(redis_conn: &mut redis::Connection, key: &str) -> Option<String> {
    redis::cmd("LINDEX")
        .arg(key)
        .arg(0)
        .query(&mut *redis_conn)
        .unwrap_or(None)
}

/// Whether the stored point is newer than the one being reported now.
///
/// GPSLogger queues points while offline and repeats each logged line, so a sample
/// that arrives out of order must not move the object backwards.
fn stored_is_newer(previous: Option<&str>, when: DateTime<Utc>) -> bool {
    let Some(stored) = previous.and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
    else {
        return false;
    };
    stored
        .get("t")
        .and_then(|value| value.as_str())
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .is_some_and(|old| old.with_timezone(&Utc) > when)
}

/// Appends the point to the object's track, caps the history and renews the key's
/// lifetime — all in one round trip, so a failed trim cannot leave the list
/// unbounded.
fn store_point(
    active: &crate::maps::ActiveMap,
    redis_conn: &mut redis::Connection,
    key: &str,
    point: &serde_json::Value,
) -> Result<(), Response<axum::body::Body>> {
    let mut pipe = redis::pipe();
    pipe.atomic();
    pipe.cmd("LPUSH").arg(key).arg(point.to_string()).ignore();
    pipe.cmd("LTRIM")
        .arg(key)
        .arg(0)
        .arg(TRACK_HISTORY_POINTS - 1)
        .ignore();
    // The TTL is what makes a stale coordinate disappear instead of lingering: a
    // tracker silent for longer than `stale_after_secs` leaves no key behind, and
    // `GET /api/geo/trackFreshness` says so.
    pipe.cmd("EXPIRE")
        .arg(key)
        .arg(active.location.stale_after_secs.max(1))
        .ignore();
    pipe.query::<()>(redis_conn)
        .map_err(|_| super::internal_error())
}

/// Publishes the snapped cell where the rest of the app reads positions from.
///
/// The track list is history; these keys are the current coordinate of an
/// engineer, vehicle or aircraft.
fn publish_position(
    active: &crate::maps::ActiveMap,
    redis_conn: &mut redis::Connection,
    pg_conn: &mut diesel::PgConnection,
    object: &TrackedObject<'_>,
    obj: &str,
    when: DateTime<Utc>,
    grid: Point,
) {
    let position_id = object.position_id(obj);
    match object.kind {
        "vehicle" => {
            let vehicle = object
                .agent
                .and_then(|item| item.vehicle_type.as_deref())
                .and_then(|name| VehicleType::try_from(name).ok())
                .unwrap_or(VehicleType::FuelTruck);
            let _ = redis_conn.set(
                active.vehicle_position_key(&vehicle, position_id),
                grid.as_value(),
            );
        }
        "aircraft" => {
            let _ = redis_conn.set(
                active.vehicle_position_key(&VehicleType::Aircraft, position_id),
                grid.as_value(),
            );
        }
        _ => {
            let _ = redis_conn.set(active.engineer_position_key(position_id), grid.as_value());
            let _ = redis_conn.set(
                active.engineer_timestamp_key(position_id),
                when.timestamp().to_string(),
            );
            // ТЗ п.20: the forecast follows the engineer, so a live position
            // refresh also refreshes the SLA forecast of their open task.
            refresh_forecast(active, redis_conn, pg_conn, position_id, grid);
        }
    }
}

/// Acknowledges a point without moving the object.
fn ignored(sample: &TrackSample, stamp: String) -> TrackAck {
    TrackAck {
        accepted: true,
        applied: false,
        obj: sample.obj.clone(),
        t: stamp,
    }
}

fn apply_point(
    active: &crate::maps::ActiveMap,
    redis_conn: &mut redis::Connection,
    pg_conn: &mut diesel::PgConnection,
    sample: TrackSample,
) -> Result<TrackAck, Response<axum::body::Body>> {
    let lat = sample.lat.unwrap_or(0.0);
    let lon = sample.lon.unwrap_or(0.0);
    let when = sample.when.unwrap_or_else(Utc::now);
    let stamp = sample.stamp();

    let Some(object) = sample.resolve(active, pg_conn) else {
        return Ok(ignored(&sample, stamp));
    };
    let key = track_key(active, &sample.obj);
    // The newest point of this object decides staleness and outliers; the rest of
    // the list is history. Objects never share a key, so one moving aircraft does
    // not displace an engineer's samples.
    let previous = previous_point(redis_conn, &key);
    if stored_is_newer(previous.as_deref(), when) {
        return Ok(ignored(&sample, stamp));
    }
    let previous_value = previous
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
    if let Some(stored) = &previous_value {
        if outlier(active, stored, lat, lon, when, sample.acc) {
            crate::journal::record(
                pg_conn,
                "track_rejected",
                Some(&sample.obj),
                None,
                serde_json::json!({ "lat": lat, "lon": lon, "acc": sample.acc, "spd": sample.spd }),
                None,
            );
            return Ok(ignored(&sample, stamp));
        }
    }
    let Some((grid, _)) = active.location.graph.snap(lat, lon, object.agent_kind()) else {
        crate::journal::record(
            pg_conn,
            "track_rejected",
            Some(&sample.obj),
            None,
            serde_json::json!({ "reason": "snap failed", "lat": lat, "lon": lon }),
            None,
        );
        return Ok(ignored(&sample, stamp));
    };
    // A pin corrects where the object is on the map, not what the tracker
    // reported: `lat`/`lon` below stay the raw values.
    let grid = match crate::calibration::load(redis_conn, &active.id, &sample.obj) {
        Some(pin) if pin.kind.agrees_with(object.agent_kind()) => pin.point,
        _ => grid,
    };
    let stored = serde_json::json!({
        "lat": lat,
        "lon": lon,
        "t": stamp,
        "x": grid.0,
        "y": grid.1,
        "acc": sample.acc,
        "spd": sample.spd,
    });
    store_point(active, redis_conn, &key, &stored)?;
    publish_position(active, redis_conn, pg_conn, &object, &sample.obj, when, grid);
    Ok(TrackAck {
        accepted: true,
        applied: true,
        obj: sample.obj,
        t: stamp,
    })
}


fn engineer_exists(conn: &mut diesel::PgConnection, id: Uuid) -> bool {
    use crate::database::schema::engineers;
    use diesel::prelude::*;
    engineers::table
        .find(id)
        .select(engineers::id)
        .first::<Uuid>(conn)
        .optional()
        .ok()
        .flatten()
        .is_some()
}

fn outlier(
    active: &crate::maps::ActiveMap,
    old: &serde_json::Value,
    lat: f64,
    lon: f64,
    when: DateTime<Utc>,
    acc: Option<f64>,
) -> bool {
    if acc.is_some_and(|metres| metres > active.location.max_accuracy_m) {
        return true;
    }
    let (Some(old_lat), Some(old_lon), Some(old_t)) = (
        old.get("lat").and_then(|value| value.as_f64()),
        old.get("lon").and_then(|value| value.as_f64()),
        old.get("t").and_then(|value| value.as_str()),
    ) else {
        return false;
    };
    let Ok(old_time) = DateTime::parse_from_rfc3339(old_t) else {
        return false;
    };
    let dt = (when - old_time.with_timezone(&Utc)).num_seconds().max(1) as f64;
    let metres = haversine_m(old_lat, old_lon, lat, lon);
    metres / dt > active.location.max_speed_mps
}

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

/// Recomputes `tasks.eta_seconds` for the engineer's open task after they moved.
///
/// Gated on `map:{id}:eta_cell:{task}`: a packet of hundreds of points inside
/// one cell runs no A\* at all, which is what keeps the tracker cheap. The
/// journal records `sla_risk`/`sla_breached` only when the forecast crosses
/// into that state, never on every point, and a failed recomputation leaves the
/// accepted point untouched.
fn refresh_forecast(
    active: &crate::maps::ActiveMap,
    redis_conn: &mut redis::Connection,
    pg_conn: &mut diesel::PgConnection,
    engineer: Uuid,
    grid: crate::types::map::Point,
) {
    use diesel::prelude::*;

    use crate::database::schema::tasks;
    use crate::models::task::Task;

    let open = tasks::table
        .filter(tasks::assigned_engineer.eq(engineer))
        .filter(tasks::is_active.eq(true))
        .filter(tasks::map_id.eq(&active.id))
        .filter(tasks::status.eq_any(["assigned", "in_progress"]))
        .select(Task::as_select())
        .load::<Task>(pg_conn);
    let Ok(tasks_of_engineer) = open else {
        return;
    };
    // An engineer can hold another open task while this one waits for parts.
    // Refreshing only the first would leave the rest with a stale forecast, so
    // every open task of the moving engineer is refreshed.
    for task in tasks_of_engineer {
        refresh_one_forecast(active, redis_conn, pg_conn, engineer, grid, &task);
    }
}

/// Refreshes the forecast of one open task, gated on the engineer's cell.
fn refresh_one_forecast(
    active: &crate::maps::ActiveMap,
    redis_conn: &mut redis::Connection,
    pg_conn: &mut diesel::PgConnection,
    engineer: Uuid,
    grid: crate::types::map::Point,
    task: &crate::models::task::Task,
) {
    use diesel::prelude::*;

    use crate::database::schema::tasks;

    let cell_key = format!("map:{}:eta_cell:{}", active.id, task.id);
    let crate::types::map::Point(cell_x, cell_y) = grid;
    let cell = format!("{cell_x},{cell_y}");
    let previous: Option<String> = redis_conn.get(&cell_key).unwrap_or(None);
    if previous.as_deref() == Some(cell.as_str()) {
        return;
    }
    let planned = active.location.graph.route_stops(
        AgentKind::Engineer,
        &[
            grid,
            crate::types::map::Point::new(task.plane_x, task.plane_y),
        ],
    );
    let Ok(planned) = planned else {
        return;
    };
    let eta = planned.seconds.ceil() as i32;
    let now = Utc::now();
    let predicted = now + chrono::Duration::seconds(i64::from(eta));
    let window = chrono::Duration::seconds(active.location.alert_before_secs as i64);
    // Risk is the same rule `list_alerts` applies, so the two agree.
    let risky = predicted > task.ends_at || now + window >= task.ends_at;
    let breached = now > task.ends_at;
    if diesel::update(tasks::table.find(task.id))
        .set(tasks::eta_seconds.eq(Some(eta)))
        .execute(pg_conn)
        .is_err()
    {
        return;
    }
    if risky && !task.sla_alerted {
        if diesel::update(tasks::table.find(task.id))
            .set(tasks::sla_alerted.eq(true))
            .execute(pg_conn)
            .is_ok()
        {
            crate::journal::record(
                pg_conn,
                "sla_risk",
                Some(&engineer.to_string()),
                Some(task.id),
                serde_json::json!({
                    "eta_seconds": eta,
                    "deadline": task.ends_at.to_rfc3339(),
                    "source": "track",
                }),
                None,
            );
        }
    }
    if breached && !task.sla_breached {
        if diesel::update(tasks::table.find(task.id))
            .set(tasks::sla_breached.eq(true))
            .execute(pg_conn)
            .is_ok()
        {
            crate::journal::record(
                pg_conn,
                "sla_breached",
                Some(&engineer.to_string()),
                Some(task.id),
                serde_json::json!({
                    "deadline": task.ends_at.to_rfc3339(),
                    "source": "track",
                }),
                None,
            );
        }
    }
    let _ = redis_conn.set(&cell_key, cell);
}
