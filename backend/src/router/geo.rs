//! Geo handlers: position updates and read endpoints for engineers, aircraft and vehicles.

use std::sync::Arc;

use axum::http::Response;
use axum::{Json, body::Body, extract::State, http::StatusCode, response::IntoResponse};
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use redis::TypedCommands;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{bad_request, internal_error};
use crate::utils::get_transport_from_redis;
use crate::{
    database::schema,
    types::{
        config::AppState,
        dto::{UpdateEngineerPositionRequest, UpdateTransportPositionRequest},
        enums::VehicleType,
        map::Point,
    },
};

// ---------------------------------------------------------------------------
// Position updates
// ---------------------------------------------------------------------------

/// Applies a buffered batch of engineer positions.
///
/// Samples may arrive out of order. The stored position becomes the sample
/// with the latest unix timestamp, and an older batch does not move the
/// engineer backwards.
#[utoipa::path(
    post,
    path = "/api/geo/updateEngineerPosition",
    description = "Flush a buffered batch of engineer positions. Each sample is [point, unix_timestamp]; the newest timestamp wins.",
    request_body = UpdateEngineerPositionRequest,
    responses(
        (status = 200, description = "Batch applied, or ignored because a newer position is already stored"),
        (status = 400, description = "Invalid UUID, empty batch, or out-of-bounds coordinate", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
)]
pub async fn update_engineer_position_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<UpdateEngineerPositionRequest>,
) -> Result<Response<Body>, Response<Body>> {
    let uuid = Uuid::parse_str(&payload.id).map_err(|_| bad_request("invalid UUID format"))?;

    if payload.batch.is_empty() {
        return Err(bad_request("batch is empty"));
    }

    let mut latest: Option<(Point, i64)> = None;
    for sample in payload.batch {
        let point = sample.0;
        let timestamp = sample.1;
        if timestamp < 0 {
            return Err(bad_request("timestamp must be a unix time in seconds"));
        }
        if !app_state.map.in_bounds(point) {
            return Err(bad_request("point is outside of map bounds"));
        }
        if latest.is_none_or(|(_, best)| timestamp >= best) {
            latest = Some((point, timestamp));
        }
    }
    let (point, timestamp) = latest.expect("batch was checked to be non-empty");

    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let ts_key = format!("position_ts:{uuid}");
    let stored_raw: Option<String> = redis_conn.get(&ts_key).map_err(|e| {
        tracing::error!(%uuid, error = %e, "failed to read engineer position timestamp");
        internal_error()
    })?;
    let stored_ts = stored_raw.and_then(|raw| raw.parse::<i64>().ok());
    if stored_ts.is_some_and(|stored| stored > timestamp) {
        tracing::debug!(%uuid, stored_ts, timestamp, "ignored stale engineer position batch");
        return Ok(StatusCode::OK.into_response());
    }

    let mut pipe = redis::pipe();
    pipe.atomic()
        .set(format!("position:{uuid}"), point.as_value())
        .ignore()
        .set(ts_key, timestamp.to_string())
        .ignore()
        .query::<()>(&mut redis_conn)
        .map_err(|e| {
            tracing::error!(%uuid, error = %e, "failed to set engineer position");
            internal_error()
        })?;

    Ok(StatusCode::OK.into_response())
}

/// Manually updates a special vehicle's position.
#[utoipa::path(
    post,
    path = "/api/geo/updateTransportPosition",
    description = "Manually update a special-vehicle position (creates the vehicle if it does not exist)",
    request_body = UpdateTransportPositionRequest,
    responses(
        (status = 200, description = "Position updated"),
        (status = 400, description = "Invalid UUID or out-of-bounds coordinate", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
)]
pub async fn update_transport_position_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<UpdateTransportPositionRequest>,
) -> Result<Response<Body>, Response<Body>> {
    let uuid = Uuid::parse_str(&payload.id).map_err(|_| bad_request("invalid UUID format"))?;

    if !app_state.map.in_bounds(payload.new_point) {
        return Err(bad_request("point is outside of map bounds"));
    }

    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let key = format!("position:{}:{}", payload.vehicle_type, uuid);
    redis_conn
        .set(key, payload.new_point.as_value())
        .map_err(|e| {
            tracing::error!(%uuid, error = %e, "failed to set transport position");
            internal_error()
        })?;

    Ok(StatusCode::OK.into_response())
}

// ---------------------------------------------------------------------------
// Read endpoints
// ---------------------------------------------------------------------------

/// Response payload for [`active_engineers`].
#[derive(Debug, Serialize, ToSchema)]
pub struct ActiveEngineersResponse {
    /// UUIDs of engineers currently assigned to an active task.
    pub active_engineers: Vec<String>,
}

/// Returns the list of engineers with at least one active task.
#[utoipa::path(
    get,
    path = "/api/geo/activeEngineers",
    description = "List engineers who currently have an active task",
    responses(
        (status = 200, description = "List of engineer UUIDs", body = ActiveEngineersResponse),
        (status = 500, description = "Server-side error", body = String),
    ),
)]
pub async fn active_engineers(
    State(app_state): State<Arc<AppState>>,
) -> Result<Response<Body>, Response<Body>> {
    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;

    let ids: Vec<Uuid> = schema::tasks::table
        .filter(schema::tasks::is_active.eq(true))
        .select(schema::tasks::assigned_engineer)
        .load(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load active engineers");
            internal_error()
        })?;

    Ok(Json(ActiveEngineersResponse {
        active_engineers: ids.into_iter().map(|u| u.to_string()).collect(),
    })
    .into_response())
}

/// Response entry for [`get_engineers_positions`].
#[derive(Debug, Serialize, ToSchema)]
pub struct EngineerPositionEntry {
    /// Engineer UUID.
    pub id: Uuid,
    /// Current position, if known.
    pub position: Option<Point>,
}

/// Returns every engineer together with their current position.
#[utoipa::path(
    get,
    path = "/api/geo/getEngineersPositions",
    description = "List all engineers and their positions",
    responses(
        (status = 200, description = "Engineer positions", body = Vec<EngineerPositionEntry>),
        (status = 500, description = "Server-side error", body = String),
    ),
)]
pub async fn get_engineers_positions(
    State(app_state): State<Arc<AppState>>,
) -> Result<Response<Body>, Response<Body>> {
    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let ids: Vec<Uuid> = schema::engineers::table
        .select(schema::engineers::id)
        .load(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load engineers");
            internal_error()
        })?;

    // One round-trip instead of N.
    let keys: Vec<String> = ids.iter().map(|id| format!("position:{}", id)).collect();

    let raw: Vec<Option<String>> = redis::cmd("MGET")
        .arg(&keys)
        .query(&mut redis_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to fetch engineer positions");
            internal_error()
        })?;

    let result: Vec<EngineerPositionEntry> = ids
        .into_iter()
        .zip(raw)
        .map(|(id, value)| EngineerPositionEntry {
            id,
            position: value.and_then(|v| Point::from_value(v).ok()),
        })
        .collect();

    Ok(Json(result).into_response())
}

/// Response entry for [`get_transport_positions`].
#[derive(Debug, Serialize, ToSchema)]
pub struct TransportPositionEntry {
    /// Vehicle UUID.
    pub id: Uuid,
    /// Vehicle type.
    pub vehicle_type: VehicleType,
    /// Current position.
    pub point: Point,
}

/// Returns every special vehicle with its type and current position.
#[utoipa::path(
    get,
    path = "/api/geo/getTransportPositions",
    description = "List all special vehicles with type, UUID and position",
    responses(
        (status = 200, description = "Transport positions", body = Vec<TransportPositionEntry>),
        (status = 500, description = "Server-side error", body = String),
    ),
)]
pub async fn get_transport_positions(
    State(app_state): State<Arc<AppState>>,
) -> Result<Response<Body>, Response<Body>> {
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let result = get_transport_from_redis(&mut redis_conn, "*".to_string()).map_err(|e| {
        tracing::error!(error = %e, "failed to fetch transport positions");
        internal_error()
    });

    Ok(Json(result.unwrap()).into_response())
}
