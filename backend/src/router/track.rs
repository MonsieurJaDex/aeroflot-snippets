//! `POST /api/v1/track` — the organiser contract for engineer, vehicle and aircraft points.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::Response;
use axum::response::IntoResponse;
use chrono::{DateTime, SecondsFormat, Utc};
use redis::TypedCommands;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{active_map, bad_request};
use crate::graph::AgentKind;
use crate::types::config::AppState;
use crate::types::enums::VehicleType;

#[derive(Debug, Deserialize, ToSchema)]
pub struct TrackPoint {
    pub obj: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub t: Option<String>,
    pub acc: Option<f64>,
    pub spd: Option<f64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TrackBatch {
    pub points: Vec<TrackPoint>,
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

#[utoipa::path(
    post,
    path = "/api/v1/track",
    tag = "Geo",
    operation_id = "ingestTrack",
    request_body = TrackPoint,
    responses(
        (status = 200, description = "Point or batch accepted", body = TrackAck),
        (status = 400, description = "lat/lon missing or not numeric"),
    ),
)]
pub async fn ingest(
    State(app_state): State<Arc<AppState>>,
    Json(body): Json<TrackBody>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    let points = match body {
        TrackBody::Many(batch) => batch.points,
        TrackBody::One(point) => vec![point],
    };
    if points
        .iter()
        .any(|point| point.lat.is_none() || point.lon.is_none())
    {
        return Err(bad_request("lat and lon are required numbers"));
    }
    let active = active_map(&app_state)?;
    let mut redis_conn = app_state
        .redis_pool
        .get()
        .map_err(|_| super::internal_error())?;
    let mut pg_conn = app_state
        .db_pool
        .get()
        .map_err(|_| super::internal_error())?;
    let mut acks = Vec::with_capacity(points.len());
    for point in points {
        acks.push(apply_point(&active, &mut redis_conn, &mut pg_conn, point)?);
    }
    if acks.len() == 1 {
        Ok(Json(acks.pop().unwrap()).into_response())
    } else {
        Ok(Json(acks).into_response())
    }
}

fn apply_point(
    active: &crate::maps::ActiveMap,
    redis_conn: &mut redis::Connection,
    pg_conn: &mut diesel::PgConnection,
    point: TrackPoint,
) -> Result<TrackAck, Response<axum::body::Body>> {
    let lat = point.lat.unwrap_or(0.0);
    let lon = point.lon.unwrap_or(0.0);
    let when = point
        .t
        .as_deref()
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|time| time.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    let stamp = when.to_rfc3339_opts(SecondsFormat::Millis, true);
    let agent = active
        .location
        .agents
        .iter()
        .find(|agent| agent.obj == point.obj);
    let linked = agent
        .and_then(|agent| agent.engineer_id.as_deref())
        .and_then(|id| Uuid::parse_str(id).ok())
        .or_else(|| Uuid::parse_str(&point.obj).ok());
    let known = agent.is_some() || linked.is_some_and(|id| engineer_exists(pg_conn, id));
    if !known {
        return Ok(TrackAck {
            accepted: true,
            applied: false,
            obj: point.obj,
            t: stamp,
        });
    }
    let key = format!("map:{}:track:{}", active.id, point.obj);
    let previous: Option<String> = redis_conn.get(&key).unwrap_or(None);
    if let Some(raw) = &previous {
        if let Ok(old) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(old_t) = old.get("t").and_then(|value| value.as_str()) {
                if let Ok(old_time) = DateTime::parse_from_rfc3339(old_t) {
                    if old_time.with_timezone(&Utc) > when {
                        return Ok(TrackAck {
                            accepted: true,
                            applied: false,
                            obj: point.obj,
                            t: stamp,
                        });
                    }
                }
            }
            if outlier(active, &old, lat, lon, when, point.acc) {
                crate::journal::record(
                    pg_conn,
                    "track_rejected",
                    Some(&point.obj),
                    None,
                    serde_json::json!({ "lat": lat, "lon": lon, "acc": point.acc, "spd": point.spd }),
                    None,
                );
                return Ok(TrackAck {
                    accepted: true,
                    applied: false,
                    obj: point.obj,
                    t: stamp,
                });
            }
        }
    }
    let kind = agent
        .map(|item| item.kind.as_str())
        .unwrap_or(if point.obj.starts_with("v-") {
            "vehicle"
        } else if point.obj.starts_with("RA-") {
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
            Some(&point.obj),
            None,
            serde_json::json!({ "reason": "snap failed", "lat": lat, "lon": lon }),
            None,
        );
        return Ok(TrackAck {
            accepted: true,
            applied: false,
            obj: point.obj,
            t: stamp,
        });
    };
    let stored = serde_json::json!({
        "lat": lat,
        "lon": lon,
        "t": stamp,
        "x": grid.0,
        "y": grid.1,
        "acc": point.acc,
        "spd": point.spd,
    });
    redis_conn
        .set(&key, stored.to_string())
        .map_err(|_| super::internal_error())?;
    let position_id =
        linked.unwrap_or_else(|| Uuid::new_v5(&Uuid::NAMESPACE_OID, point.obj.as_bytes()));
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
        obj: point.obj,
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
