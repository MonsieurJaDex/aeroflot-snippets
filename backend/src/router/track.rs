//! `/api/v1/track` — the organiser contract for engineer, vehicle and aircraft
//! points. Accepts a JSON body and the query string a GPSLogger *Custom URL*
//! callback sends, both reduced to the same [`TrackSample`].

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{FromRequest, Query, Request, State};
use axum::http::{Response, StatusCode, header};
use axum::response::IntoResponse;
use chrono::{DateTime, SecondsFormat, Utc};
use redis::TypedCommands;
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::{active_map, bad_request};
use crate::graph::AgentKind;
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

fn apply_point(
    active: &crate::maps::ActiveMap,
    redis_conn: &mut redis::Connection,
    pg_conn: &mut diesel::PgConnection,
    sample: TrackSample,
) -> Result<TrackAck, Response<axum::body::Body>> {
    let TrackSample {
        obj,
        lat,
        lon,
        when,
        acc,
        spd,
    } = sample;
    let lat = lat.unwrap_or(0.0);
    let lon = lon.unwrap_or(0.0);
    let when = when.unwrap_or_else(Utc::now);
    let stamp = when.to_rfc3339_opts(SecondsFormat::Millis, true);
    let agent = active
        .location
        .agents
        .iter()
        .find(|agent| agent.obj == obj);
    let linked = agent
        .and_then(|agent| agent.engineer_id.as_deref())
        .and_then(|id| Uuid::parse_str(id).ok())
        .or_else(|| Uuid::parse_str(&obj).ok());
    let known = agent.is_some() || linked.is_some_and(|id| engineer_exists(pg_conn, id));
    if !known {
        return Ok(TrackAck {
            accepted: true,
            applied: false,
            obj: obj.clone(),
            t: stamp,
        });
    }
    let key = format!("map:{}:track:{}", active.id, obj);
    let previous: Option<String> = redis_conn.get(&key).unwrap_or(None);
    if let Some(raw) = &previous {
        if let Ok(old) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(old_t) = old.get("t").and_then(|value| value.as_str()) {
                if let Ok(old_time) = DateTime::parse_from_rfc3339(old_t) {
                    if old_time.with_timezone(&Utc) > when {
                        return Ok(TrackAck {
                            accepted: true,
                            applied: false,
                            obj: obj.clone(),
                            t: stamp,
                        });
                    }
                }
            }
            if outlier(active, &old, lat, lon, when, acc) {
                crate::journal::record(
                    pg_conn,
                    "track_rejected",
                    Some(&obj),
                    None,
                    serde_json::json!({ "lat": lat, "lon": lon, "acc": acc, "spd": spd }),
                    None,
                );
                return Ok(TrackAck {
                    accepted: true,
                    applied: false,
                    obj: obj.clone(),
                    t: stamp,
                });
            }
        }
    }
    let kind = agent
        .map(|item| item.kind.as_str())
        .unwrap_or(if obj.starts_with("v-") {
            "vehicle"
        } else if obj.starts_with("RA-") {
            "aircraft"
        } else {
            "engineer"
        });
    let agent_kind = if kind == "vehicle" {
        AgentKind::Vehicle
    } else {
        AgentKind::Engineer
    };
    let Some((grid, _)) = active.location.graph.snap(lat, lon, agent_kind) else {
        crate::journal::record(
            pg_conn,
            "track_rejected",
            Some(&obj),
            None,
            serde_json::json!({ "reason": "snap failed", "lat": lat, "lon": lon }),
            None,
        );
        return Ok(TrackAck {
            accepted: true,
            applied: false,
            obj: obj.clone(),
            t: stamp,
        });
    };
    // A pin corrects where the object is on the map, not what the tracker
    // reported: `lat`/`lon` below stay the raw values.
    let grid = match crate::calibration::load(redis_conn, &active.id, &obj) {
        Some(pin) if pin.kind.agrees_with(agent_kind) => pin.point,
        _ => grid,
    };
    let stored = serde_json::json!({
        "lat": lat,
        "lon": lon,
        "t": stamp,
        "x": grid.0,
        "y": grid.1,
        "acc": acc,
        "spd": spd,
    });
    redis_conn
        .set(&key, stored.to_string())
        .map_err(|_| super::internal_error())?;
    let position_id =
        linked.unwrap_or_else(|| Uuid::new_v5(&Uuid::NAMESPACE_OID, obj.as_bytes()));
    match kind {
        "vehicle" => {
            let vehicle = agent
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
        }
    }
    Ok(TrackAck {
        accepted: true,
        applied: true,
        obj,
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
