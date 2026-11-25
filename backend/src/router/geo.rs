//! Geo handlers: position updates and read endpoints for engineers, aircraft and vehicles.

use std::sync::Arc;

use axum::http::Response;
use axum::{Extension, Json, body::Body, extract::State, http::StatusCode, response::IntoResponse};
use diesel::{BoolExpressionMethods, ExpressionMethods, QueryDsl, RunQueryDsl};
use redis::TypedCommands;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    ApiJson, ApiQuery, active_map, bad_request, forbidden, internal_error, not_found, resolve_page,
    slice_page,
};
use crate::middleware::AuthUser;
use crate::utils::get_transport_from_redis;
use crate::{
    database::schema,
    types::{
        config::AppState,
        dto::{
            CalibrateObjectRequest, CalibrationEntry, CalibrationListResponse,
            ForceVehicleRequest, ForceVehicleResponse, PauseAgentRequest, PauseAgentResponse,
            TrackFreshnessQuery, TrackFreshnessResponse,
            StartSimulationRequest, StartSimulationResponse, StopSimulationRequest,
            StopSimulationResponse, SimulationStatusQuery, SimulationStatusResponse,
            FractionalCell,
            ClearCalibrationRequest, Coordinate, ErrorResponse, PageQuery,
            UpdateEngineerPositionRequest, UpdateTransportPositionRequest,
        },
        enums::{ObjectKind, UserRole, VehicleType},
        map::Point,
    },
    calibration,
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
    tag = "Geo",
    operation_id = "updateEngineerPosition",
    description = "Flush a buffered batch of engineer positions. Each sample is [point, unix_timestamp]; the newest timestamp wins.",
    security(("bearerAuth" = [])),
    request_body = UpdateEngineerPositionRequest,
    responses(
        (status = 200, description = "Batch applied, or ignored because a newer position is already stored"),
        (status = 400, description = "Invalid UUID, empty batch, or out-of-bounds coordinate", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not an engineer, or the batch names another engineer", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn update_engineer_position_handler(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<UpdateEngineerPositionRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Engineer {
        return Err(forbidden("only engineers can update an engineer position"));
    }
    let uuid = Uuid::parse_str(&payload.id).map_err(|_| bad_request("invalid UUID format"))?;
    if uuid != auth_user.id {
        return Err(forbidden("engineers may only update their own position"));
    }

    if payload.batch.is_empty() {
        return Err(bad_request("batch is empty"));
    }

    let active = active_map(&app_state)?;
    let mut latest: Option<(Point, i64)> = None;
    for sample in payload.batch {
        let point = sample.0;
        let timestamp = sample.1;
        if timestamp < 0 {
            return Err(bad_request("timestamp must be a unix time in seconds"));
        }
        if !active.map.in_bounds(point) {
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

    let ts_key = active.engineer_timestamp_key(uuid);
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
        .set(active.engineer_position_key(uuid), point.as_value())
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

/// Pins a tracked object to a known cell of the active map.
///
/// The tracker reports raw WGS-84 and the graph snaps it to the nearest
/// pavement cell. When that snap keeps landing in the wrong place GNSS will not
/// improve, so an operator pins the object where it actually is: the ingest then
/// stores the pin instead of the snapped cell, while the reported `lat`/`lon`
/// stay untouched.
#[utoipa::path(
    post,
    path = "/api/geo/calibrateObject",
    tag = "Geo",
    operation_id = "calibrateObject",
    description = "Pin an object to a graph cell, given either as a cell or as WGS-84 degrees",
    security(("bearerAuth" = [])),
    request_body = CalibrateObjectRequest,
    responses(
        (status = 200, description = "Pin stored", body = CalibrationEntry),
        (status = 400, description = "Neither or both of `point` and `lat`+`lon`, a cell outside the map, or a cell without a graph node", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn calibrate_object_handler(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<CalibrateObjectRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can calibrate objects"));
    }

    let point = match (payload.point, payload.lat, payload.lon) {
        (Some(point), None, None) => point,
        (None, Some(lat), Some(lon)) => {
            let active = active_map(&app_state)?;
            let (x, y) = active.location.graph.unproject(lat, lon);
            Point(x.round() as i64, y.round() as i64)
        }
        _ => {
            return Err(bad_request(
                "give either point, or lat and lon, and not both",
            ));
        }
    };

    let active = active_map(&app_state)?;
    if !active.map.in_bounds(point) {
        return Err(bad_request("point is outside of map bounds"));
    }
    if !active.location.graph.is_node(point) {
        return Err(bad_request(
            "point carries no graph node, so a route cannot start or end on it",
        ));
    }

    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    if payload.kind != ObjectKind::from_obj(&payload.obj) {
        // Not an error: the tracker's `obj` may differ from the identity the
        // caller pins. It is still worth a line, because a pin whose kind never
        // agrees with the ingest layer will not be applied.
        tracing::warn!(
            obj = %payload.obj,
            kind = ?payload.kind,
            "calibration kind does not match the object label"
        );
    }
    let calibration = calibration::Calibration {
        obj: payload.obj.clone(),
        kind: payload.kind,
        point,
    };
    calibration::save(&mut redis_conn, &active.id, &calibration).map_err(|e| {
        tracing::error!(obj = %payload.obj, error = %e, "failed to store object calibration");
        internal_error()
    })?;

    Ok(Json(calibration_entry(&active, calibration)).into_response())
}

/// Drops the pin of an object, so it snaps to its reported position again.
#[utoipa::path(
    post,
    path = "/api/geo/clearObjectCalibration",
    tag = "Geo",
    operation_id = "clearObjectCalibration",
    description = "Remove an object's calibration pin",
    security(("bearerAuth" = [])),
    request_body = ClearCalibrationRequest,
    responses(
        (status = 204, description = "Pin removed"),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 404, description = "The object carries no pin", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn clear_object_calibration_handler(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<ClearCalibrationRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can calibrate objects"));
    }

    let active = active_map(&app_state)?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let removed = calibration::clear(&mut redis_conn, &active.id, &payload.obj).map_err(|e| {
        tracing::error!(obj = %payload.obj, error = %e, "failed to drop object calibration");
        internal_error()
    })?;
    if !removed {
        return Err(not_found("object carries no calibration"));
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Returns every pin stored for the active map.
#[utoipa::path(
    get,
    path = "/api/geo/getObjectCalibrations",
    tag = "Geo",
    operation_id = "getObjectCalibrations",
    description = "List object calibration pins of the active map",
    security(("bearerAuth" = [])),
    params(PageQuery),
    responses(
        (status = 200, description = "Pins of the active map", body = CalibrationListResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 422, description = "Invalid page window", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn get_object_calibrations(
    State(app_state): State<Arc<AppState>>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Response<Body>, Response<Body>> {
    let (limit, offset) = resolve_page(query)?;
    let active = active_map(&app_state)?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let calibrations =
        calibration::list(&mut redis_conn, &active.id).map_err(|e| {
            tracing::error!(error = %e, "failed to list object calibrations");
            internal_error()
        })?;
    let entries: Vec<CalibrationEntry> = calibrations
        .into_iter()
        .map(|calibration| calibration_entry(&active, calibration))
        .collect();
    let (calibrations, total) = slice_page(entries, limit, offset);

    Ok(Json(CalibrationListResponse {
        calibrations,
        total,
    })
    .into_response())
}

/// Builds the wire form of a stored pin.
fn calibration_entry(
    active: &crate::maps::ActiveMap,
    calibration: calibration::Calibration,
) -> CalibrationEntry {
    let point = calibration.point;
    let (lat, lon) = active.location.graph.project(point.0, point.1);
    CalibrationEntry {
        obj: calibration.obj,
        kind: calibration.kind,
        point,
        geo: Coordinate { lat, lon },
        on_graph: active.location.graph.is_node(point),
    }
}

/// Manually updates a special vehicle's position.
#[utoipa::path(
    post,
    path = "/api/geo/updateTransportPosition",
    tag = "Geo",
    operation_id = "updateTransportPosition",
    description = "Manually update a special-vehicle position (creates the vehicle if it does not exist)",
    security(("bearerAuth" = [])),
    request_body = UpdateTransportPositionRequest,
    responses(
        (status = 200, description = "Position updated"),
        (status = 400, description = "Invalid UUID or out-of-bounds coordinate", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn update_transport_position_handler(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<UpdateTransportPositionRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden(
            "only dispatchers can update a transport position",
        ));
    }
    let uuid = Uuid::parse_str(&payload.id).map_err(|_| bad_request("invalid UUID format"))?;

    let active = active_map(&app_state)?;
    if !active.map.in_bounds(payload.new_point) {
        return Err(bad_request("point is outside of map bounds"));
    }

    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let key = active.vehicle_position_key(&payload.vehicle_type, uuid);
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
    tag = "Geo",
    operation_id = "getActiveEngineers",
    description = "List engineers who currently have an active task",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "List of engineer UUIDs", body = ActiveEngineersResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn active_engineers(
    State(app_state): State<Arc<AppState>>,
) -> Result<Response<Body>, Response<Body>> {
    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;

    let active = active_map(&app_state)?;
    let ids: Vec<String> = schema::tasks::table
        .filter(schema::tasks::is_active.eq(true))
        .filter(schema::tasks::status.eq_any(["assigned", "in_progress"]))
        .filter(
            schema::tasks::map_id
                .is_null()
                .or(schema::tasks::map_id.eq(active.id)),
        )
        .select(schema::tasks::assigned_engineer)
        .load::<Option<Uuid>>(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load active engineers");
            internal_error()
        })?
        .into_iter()
        .flatten()
        .map(|id| id.to_string())
        .collect();

    Ok(Json(ActiveEngineersResponse {
        active_engineers: ids,
    })
    .into_response())
}

/// Answer whether a live coordinate is stored for an object.
///
/// The coordinate itself lives in Redis under `map:{id}:track:{obj}` and expires
/// `stale_after_secs` after the last reported point, so a caller that only needs
/// "is this position current" answers that question with one boolean instead of
/// reading and parsing the whole history.
#[utoipa::path(
    get,
    path = "/api/geo/trackFreshness",
    tag = "Geo",
    operation_id = "getTrackFreshness",
    description = "Say whether a live coordinate is stored for an object",
    security(("bearerAuth" = [])),
    params(("obj" = String, Query, description = "Object a tracker reports")),
    responses(
        (status = 200, description = "Freshness of the coordinate", body = TrackFreshnessResponse),
        (status = 400, description = "Blank obj", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 422, description = "obj is missing", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn track_freshness(
    State(app_state): State<Arc<AppState>>,
    ApiQuery(query): ApiQuery<TrackFreshnessQuery>,
) -> Result<Response<Body>, Response<Body>> {
    if query.obj.trim().is_empty() {
        return Err(bad_request("obj must not be blank"));
    }
    let active = active_map(&app_state)?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let key = format!("map:{}:track:{}", active.id, query.obj);
    let mut conn = &mut *redis_conn;
    // The key's own lifetime is the freshness answer: it exists only while the
    // last point is younger than `stale_after_secs`.
    let ttl: i64 = redis::cmd("TTL")
        .arg(&key)
        .query(&mut conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to read track ttl");
            internal_error()
        })?;

    Ok(Json(TrackFreshnessResponse {
        obj: query.obj,
        fresh: ttl > 0,
        expires_in_secs: ttl.max(0),
    })
    .into_response())
}

/// Stop an agent without ending its route, or let it move again.
///
/// The flag lives in Redis next to the track (`map:{id}:paused:{obj}`), so a paused
/// object keeps its history and its last cell while incoming points are accepted
/// but not applied.
#[utoipa::path(
    post,
    path = "/api/geo/pauseAgent",
    tag = "Geo",
    operation_id = "pauseAgent",
    description = "Pause or resume an agent without ending its route",
    security(("bearerAuth" = [])),
    request_body = PauseAgentRequest,
    responses(
        (status = 200, description = "New state of the agent", body = PauseAgentResponse),
        (status = 400, description = "Blank obj", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn pause_agent_handler(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<PauseAgentRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can pause agents"));
    }
    if payload.obj.trim().is_empty() {
        return Err(bad_request("obj must not be blank"));
    }
    let active = active_map(&app_state)?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;
    let key = super::track::paused_key(&active.id, &payload.obj);
    // A running agent carries no flag at all, so resuming cannot leave a stale
    // "1" behind for the next pause to misread.
    let stored = if payload.paused {
        redis_conn.set(&key, "1").map(|()| ())
    } else {
        redis_conn.del(&key).map(|_: usize| ())
    };
    stored.map_err(|e| {
        tracing::error!(error = %e, "failed to store agent pause");
        internal_error()
    })?;
    Ok(Json(PauseAgentResponse {
        obj: payload.obj,
        paused: payload.paused,
    })
    .into_response())
}

/// Force an engineer to use a vehicle, or restore the normal walking comparison.
#[utoipa::path(
    post,
    path = "/api/geo/forceVehicle",
    tag = "Geo",
    operation_id = "forceVehicle",
    description = "Always send an engineer by vehicle when ranking candidates",
    security(("bearerAuth" = [])),
    request_body = ForceVehicleRequest,
    responses(
        (status = 200, description = "New state of the flag", body = ForceVehicleResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 404, description = "Engineer does not exist", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn force_vehicle_handler(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<ForceVehicleRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can force a vehicle"));
    }
    let mut conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire database connection");
        internal_error()
    })?;
    if !super::track::engineer_exists(&mut conn, payload.engineer_id) {
        return Err(not_found("engineer not found"));
    }
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;
    let key = crate::rank::forced_vehicle_key(payload.engineer_id);
    let stored = if payload.forced {
        redis_conn.set(&key, "1").map(|()| ())
    } else {
        redis_conn.del(&key).map(|_: usize| ())
    };
    stored.map_err(|e| {
        tracing::error!(error = %e, "failed to store forced vehicle flag");
        internal_error()
    })?;
    Ok(Json(ForceVehicleResponse {
        engineer_id: payload.engineer_id,
        forced: payload.forced,
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
    /// True when the last point is older than the location stale window.
    pub stale: bool,
    /// Timestamp of the last accepted point.
    #[schema(nullable, format = "date-time")]
    pub updated_at: Option<String>,
}

/// Engineers and their positions, windowed by `limit` and `offset`.
#[derive(Debug, Serialize, ToSchema)]
#[schema(description = "Page of engineer positions.")]
pub struct EngineersPositionsResponse {
    pub engineers: Vec<EngineerPositionEntry>,
    /// Number of engineers before `limit` and `offset` are applied.
    pub total: u64,
}

/// Returns every engineer together with their current position.
#[utoipa::path(
    get,
    path = "/api/geo/getEngineersPositions",
    tag = "Geo",
    operation_id = "getEngineersPositions",
    description = "List engineers and their positions",
    security(("bearerAuth" = [])),
    params(PageQuery),
    responses(
        (status = 200, description = "Engineer positions", body = EngineersPositionsResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 422, description = "Query parameters do not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn get_engineers_positions(
    State(app_state): State<Arc<AppState>>,
    ApiQuery(page): ApiQuery<PageQuery>,
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
    let active = active_map(&app_state)?;
    let keys: Vec<String> = ids
        .iter()
        .map(|id| active.engineer_position_key(*id))
        .collect();

    let raw: Vec<Option<String>> = redis::cmd("MGET")
        .arg(&keys)
        .query(&mut redis_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to fetch engineer positions");
            internal_error()
        })?;

    let (limit, offset) = resolve_page(page)?;
    let result: Vec<EngineerPositionEntry> = ids
        .into_iter()
        .zip(raw)
        .map(|(id, value)| EngineerPositionEntry {
            id,
            position: value.and_then(|v| Point::from_value(v).ok()),
            stale: false,
            updated_at: None,
        })
        .collect();
    let (engineers, total) = slice_page(result, limit, offset);

    Ok(Json(EngineersPositionsResponse { engineers, total }).into_response())
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

/// Special vehicles and their positions, windowed by `limit` and `offset`.
#[derive(Debug, Serialize, ToSchema)]
#[schema(description = "Page of special-vehicle positions.")]
pub struct TransportPositionsResponse {
    pub transport: Vec<TransportPositionEntry>,
    /// Number of vehicles before `limit` and `offset` are applied.
    pub total: u64,
}

/// Returns every special vehicle with its type and current position.
#[utoipa::path(
    get,
    path = "/api/geo/getTransportPositions",
    tag = "Geo",
    operation_id = "getTransportPositions",
    description = "List special vehicles with type, UUID and position",
    security(("bearerAuth" = [])),
    params(PageQuery),
    responses(
        (status = 200, description = "Transport positions", body = TransportPositionsResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 422, description = "Query parameters do not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn get_transport_positions(
    State(app_state): State<Arc<AppState>>,
    ApiQuery(page): ApiQuery<PageQuery>,
) -> Result<Response<Body>, Response<Body>> {
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let (limit, offset) = resolve_page(page)?;
    let active = active_map(&app_state)?;
    let result =
        get_transport_from_redis(&mut redis_conn, &active.id, "*".to_string()).map_err(|e| {
            tracing::error!(error = %e, "failed to fetch transport positions");
            internal_error()
        })?;
    let (transport, total) = slice_page(result, limit, offset);

    Ok(Json(TransportPositionsResponse { transport, total }).into_response())
}

// ---------------------------------------------------------------------------
// Simulation of movement
// ---------------------------------------------------------------------------

/// Starts an object moving along a route the road graph plans.
///
/// The route is planned once, here, with the graph's per-segment speeds, and
/// then driven by the server: [`crate::simulation::drive`] republishes the
/// position every `tick_ms` through the same keys a tracker report writes, so the
/// dispatcher map, the track history and the SLA forecast see one movement.
///
/// The start of the movement is journalled as `simulation_started` — the
/// organiser checklist asks for the beginning of a movement to be in the journal,
/// and this is the moment the route is committed to.
#[utoipa::path(
    post,
    path = "/api/geo/startSimulation",
    tag = "Geo",
    operation_id = "startSimulation",
    description = "Plan a route over the road graph and move the object along it, republishing its position with smooth coordinates",
    security(("bearerAuth" = [])),
    request_body = StartSimulationRequest,
    responses(
        (status = 200, description = "Movement started; the answer carries the plan", body = StartSimulationResponse),
        (status = 400, description = "Blank obj, fewer than two stops, or no route between them", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 404, description = "Object is unknown to the map, the database and the id shapes", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn start_simulation_handler(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<StartSimulationRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can move agents"));
    }
    if payload.obj.trim().is_empty() {
        return Err(bad_request("obj must not be blank"));
    }
    if payload.stops.len() < 2 {
        return Err(bad_request("stops must name at least a start and an end"));
    }
    if payload.tick_ms.is_some_and(|tick| tick < 50) {
        return Err(bad_request("tick_ms must be at least 50"));
    }
    if payload
        .speed_scale
        .is_some_and(|scale| !(scale.is_finite() && scale > 0.0))
    {
        return Err(bad_request("speed_scale must be a positive number"));
    }
    let active = active_map(&app_state)?;
    let agent = crate::simulation::agent_of(&payload.obj);
    let route = active
        .location
        .graph
        .route_stops(agent, &payload.stops)
        .map_err(|error| bad_request(format!("no route: {error}")))?;
    if route.points.len() < 2 || route.seconds <= 0.0 {
        return Err(bad_request("the stops are the same cell: nothing to move"));
    }

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    // The object must be one the tracker would accept, otherwise the driver would
    // publish into nothing and the caller would wait for a movement never seen.
    let sample = crate::router::track::TrackSample {
        obj: payload.obj.clone(),
        lat: None,
        lon: None,
        when: None,
        acc: None,
        spd: None,
    };
    if sample.resolve(&active, &mut pg_conn).is_none() {
        return Err(not_found("object is unknown to this map and database"));
    }

    let tick_ms = payload.tick_ms.unwrap_or(crate::simulation::DEFAULT_TICK_MS);
    let speed_scale = payload.speed_scale.unwrap_or(1.0);
    let started_at = chrono::Utc::now();
    let simulation = crate::simulation::Simulation {
        obj: payload.obj.clone(),
        route: route.clone(),
        clock_ms: 0,
        started_at_ms: started_at.timestamp_millis(),
        speed_scale,
        tick_ms,
    };
    crate::simulation::save(&mut redis_conn, &active.id, &simulation).map_err(|e| {
        tracing::error!(error = %e, "failed to store simulation");
        internal_error()
    })?;

    // ТЗ requires the beginning of a movement to be in the journal; it is written
    // at the moment the route is committed to, before the first tick.
    crate::journal::record(
        &mut pg_conn,
        "simulation_started",
        Some(&payload.obj),
        None,
        serde_json::json!({
            "obj": payload.obj,
            "stops": payload.stops,
            "points": route.points.len(),
            "seconds": route.seconds,
            "tick_ms": tick_ms,
            "speed_scale": speed_scale,
        }),
        None,
    );

    tokio::spawn(crate::simulation::drive(
        Arc::clone(&app_state),
        payload.obj.clone(),
    ));

    Ok(Json(StartSimulationResponse {
        obj: payload.obj,
        points: route.points,
        seconds: route.seconds,
        started_at: started_at.to_rfc3339(),
        tick_ms,
        speed_scale,
    })
    .into_response())
}

/// Ends a movement where it stands. The object keeps the position it reached and
/// its route is dropped, so it can be given a new one.
#[utoipa::path(
    post,
    path = "/api/geo/stopSimulation",
    tag = "Geo",
    operation_id = "stopSimulation",
    description = "Stop a running simulation; the object keeps its last position",
    security(("bearerAuth" = [])),
    request_body = StopSimulationRequest,
    responses(
        (status = 200, description = "Movement stopped, or none was running", body = StopSimulationResponse),
        (status = 400, description = "Blank obj", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn stop_simulation_handler(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<StopSimulationRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can stop agents"));
    }
    if payload.obj.trim().is_empty() {
        return Err(bad_request("obj must not be blank"));
    }
    let active = active_map(&app_state)?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;
    let running = crate::simulation::load(&mut redis_conn, &active.id, &payload.obj);
    if let Some(simulation) = &running {
        let sample = simulation.sample();
        crate::simulation::clear(&mut redis_conn, &active.id, &payload.obj).map_err(|e| {
            tracing::error!(error = %e, "failed to clear simulation");
            internal_error()
        })?;
        if let Ok(mut pg_conn) = app_state.db_pool.get() {
            crate::journal::record(
                &mut pg_conn,
                "simulation_stopped",
                Some(&payload.obj),
                None,
                serde_json::json!({
                    "obj": payload.obj,
                    "x": sample.x,
                    "y": sample.y,
                    "remaining_seconds": sample.remaining_seconds,
                }),
                None,
            );
        }
    }
    let point = redis_conn
        .get(crate::simulation::smooth_key(&active.id, &payload.obj))
        .ok()
        .flatten()
        .and_then(|raw| {
            let (x, y) = raw.split_once(',')?;
            Some(FractionalCell {
                x: x.trim().parse().ok()?,
                y: y.trim().parse().ok()?,
            })
        });
    Ok(Json(StopSimulationResponse {
        obj: payload.obj,
        stopped: running.is_some(),
        point,
    })
    .into_response())
}

/// Where a simulated object is right now.
///
/// The fractional cell is the smooth coordinate; it exists as long as the object
/// has moved, whether or not a route is still being driven, so a finished journey
/// can still be inspected.
#[utoipa::path(
    get,
    path = "/api/geo/simulationStatus",
    tag = "Geo",
    operation_id = "simulationStatus",
    description = "Whether the object is moving, its fractional cell in WGS-84 and the route time left",
    security(("bearerAuth" = [])),
    params(SimulationStatusQuery),
    responses(
        (status = 200, description = "State of the object", body = SimulationStatusResponse),
        (status = 400, description = "Blank obj", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 422, description = "Query parameters do not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn simulation_status_handler(
    State(app_state): State<Arc<AppState>>,
    ApiQuery(query): ApiQuery<SimulationStatusQuery>,
) -> Result<Response<Body>, Response<Body>> {
    if query.obj.trim().is_empty() {
        return Err(bad_request("obj must not be blank"));
    }
    let active = active_map(&app_state)?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;
    let simulation = crate::simulation::load(&mut redis_conn, &active.id, &query.obj);
    let paused = super::track::is_paused(&mut redis_conn, &active.id, &query.obj);

    // A stored simulation is authoritative while it runs; afterwards the `smooth`
    // key is what is left of the journey.
    let (x, y, remaining, total) = match &simulation {
        Some(running) => {
            let sample = running.sample();
            (
                Some(sample.x),
                Some(sample.y),
                sample.remaining_seconds,
                running.route.seconds,
            )
        }
        None => (None, None, 0.0, 0.0),
    };
    let (x, y) = match (x, y) {
        (Some(x), Some(y)) => (Some(x), Some(y)),
        _ => redis_conn
            .get(crate::simulation::smooth_key(&active.id, &query.obj))
            .ok()
            .flatten()
            .and_then(|raw| {
                let (x, y) = raw.split_once(',')?;
                Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
            })
            .map_or((None, None), |(x, y)| (Some(x), Some(y))),
    };
    let (lat, lon) = match (x, y) {
        (Some(x), Some(y)) => {
            let (lat, lon) = active.location.graph.project_f64(x, y);
            (Some(lat), Some(lon))
        }
        _ => (None, None),
    };
    Ok(Json(SimulationStatusResponse {
        obj: query.obj,
        active: simulation.is_some(),
        x,
        y,
        lat,
        lon,
        remaining_seconds: remaining,
        paused,
        total_seconds: total,
    })
    .into_response())
}
