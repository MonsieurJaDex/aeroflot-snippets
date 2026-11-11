//! Simulation handlers: manual position updates and read endpoints.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use axum::{
    Json,
    body::Body,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
};
use axum::http::Response;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use redis::TypedCommands;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{bad_request, internal_error};
use crate::{
    database::schema,
    types::{
        config::AppState,
        dto::{UpdateEngineerPositionRequest, UpdateTransportPositionRequest},
        enums::SpecialVehicle,
        map::Point,
    },
};

// ---------------------------------------------------------------------------
// Position updates
// ---------------------------------------------------------------------------

/// Manually updates an engineer's position.
#[utoipa::path(
    post,
    path = "/api/simulate/update_engineer_position",
    description = "Manually update an engineer's position (simulation only)",
    request_body = UpdateEngineerPositionRequest,
    responses(
        (status = 200, description = "Position updated"),
        (status = 400, description = "Invalid UUID or out-of-bounds coordinate", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
)]
pub async fn update_engineer_position_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<UpdateEngineerPositionRequest>,
) -> Result<Response<Body>, Response<Body>> {
    let uuid = Uuid::parse_str(&payload.id)
        .map_err(|_| bad_request("invalid UUID format"))?;

    if !app_state.map.in_bounds(payload.new_point) {
        return Err(bad_request("point is outside of map bounds"));
    }

    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    redis_conn
        .set(format!("position:{}", uuid), payload.new_point.as_value())
        .map_err(|e| {
            tracing::error!(%uuid, error = %e, "failed to set engineer position");
            internal_error()
        })?;

    Ok(StatusCode::OK.into_response())
}

/// Manually updates a special vehicle's position.
#[utoipa::path(
    post,
    path = "/api/simulate/update_transport_position",
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
    let uuid = Uuid::parse_str(&payload.id)
        .map_err(|_| bad_request("invalid UUID format"))?;

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
    path = "/api/simulate/active_engineers",
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
    path = "/api/simulate/get_engineers_positions",
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
    let keys: Vec<String> = ids
        .iter()
        .map(|id| format!("position:{}", id))
        .collect();

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
    pub vehicle_type: SpecialVehicle,
    /// Current position.
    pub point: Point,
}

/// Returns every special vehicle with its type and current position.
#[utoipa::path(
    get,
    path = "/api/simulate/get_transport_positions",
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

    let keys: Vec<String> = redis_conn
        .scan_match("position:*:*")
        .map_err(|e| {
            tracing::error!(error = %e, "failed to scan transport keys");
            internal_error()
        })?
        .filter_map(Result::ok)
        .collect();

    let mut result = Vec::with_capacity(keys.len());

    for key in keys {
        // Key shape: position:{vehicle_type}:{uuid}
        let mut parts = key.splitn(3, ':');
        let _prefix = parts.next();
        let (Some(type_str), Some(uuid_str)) = (parts.next(), parts.next()) else {
            tracing::warn!(%key, "malformed transport key, skipped");
            continue;
        };

        let Ok(vehicle_type) = SpecialVehicle::try_from(type_str) else {
            tracing::warn!(%key, type_str, "unknown vehicle type, skipped");
            continue;
        };
        let Ok(id) = Uuid::from_str(uuid_str) else {
            tracing::warn!(%key, "malformed transport uuid, skipped");
            continue;
        };

        let Some(value) = redis_conn.get(&key).map_err(|e| {
            tracing::error!(%key, error = %e, "failed to fetch transport position");
            internal_error()
        })? else {
            continue; // key disappeared between SCAN and GET
        };

        let Ok(point) = Point::from_value(value) else {
            tracing::warn!(%key, "malformed transport position, skipped");
            continue;
        };

        result.push(TransportPositionEntry {
            id,
            vehicle_type,
            point,
        });
    }

    Ok(Json(result).into_response())
}