//! HTTP router: common helpers, state and public handler definitions.

pub mod auth;
pub mod geo;
pub mod ops;
pub mod track;

use std::{collections::HashMap, str::FromStr, sync::Arc};

use axum::extract::rejection::JsonRejection;
use axum::{
    body::Body,
    extract::{FromRequest, FromRequestParts, Request, State},
    http::{request::Parts, Response, StatusCode},
    response::IntoResponse,
    Extension, Json,
};

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use redis::TypedCommands;
use uuid::Uuid;

use crate::{
    database::schema::{aircraft_movements, engineers, tasks},
    maps::ActiveMap,
    middleware::AuthUser,
    models::{aircraft_movement::AircraftMovement, task::Task},
    types::{
        config::AppState,
        dto::{
            ActiveMapResponse, AircraftEntry, AircraftListResponse, AircraftMovementListResponse,
            AircraftMovementResponse, AogListResponse, ArriveAircraftRequest,
            AssignCandidatesRequest, AssignEngineerRequest, AssignEngineerResponse,
            AssignmentCandidate,
            AssignmentCandidatesResponse, ClassifyRequest, ClassifyResponse, CurrentTaskAogRequest,
            CurrentTaskAogResponse, CurrentTaskResponse, DepartAircraftRequest, ErrorResponse,
            GetRouteRequest, GetRouteResponse, MapListResponse, MapResponse, MapSummary, PageQuery,
            ScoreFactor, SelectMapRequest, SelectionBasis, TrainClassifierResponse,
        },
        enums::{UserRole, VehicleType},
        map::{Point, Route},
    },
};

// ---------------------------------------------------------------------------
// Shared response helpers
// ---------------------------------------------------------------------------

pub(crate) fn api_error(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
) -> Response<Body> {
    (
        status,
        Json(ErrorResponse {
            code: code.to_owned(),
            message: message.into(),
            details: None,
            request_id: None,
        }),
    )
        .into_response()
}

/// Returns a 500 JSON error.
pub(crate) fn internal_error() -> Response<Body> {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL_ERROR",
        "internal server error",
    )
}

/// Returns a 400 JSON error for a business-rule failure.
pub(crate) fn bad_request(msg: impl Into<String>) -> Response<Body> {
    api_error(StatusCode::BAD_REQUEST, "BAD_REQUEST", msg)
}

/// Returns a 422 JSON error for a body or query that does not match the schema.
pub(crate) fn unprocessable(msg: impl Into<String>) -> Response<Body> {
    api_error(StatusCode::UNPROCESSABLE_ENTITY, "VALIDATION_ERROR", msg)
}

/// Returns a 401 JSON error.
pub(crate) fn unauthorized(msg: impl Into<String>) -> Response<Body> {
    let message = msg.into();
    let code = if message.contains("email or password") {
        "INVALID_CREDENTIALS"
    } else {
        "UNAUTHORIZED"
    };
    api_error(StatusCode::UNAUTHORIZED, code, message)
}

/// Returns a 403 JSON error.
pub(crate) fn forbidden(msg: impl Into<String>) -> Response<Body> {
    let message = msg.into();
    let code = if message.contains("dispatcher") {
        "NOT_A_DISPATCHER"
    } else {
        "NOT_AN_ENGINEER"
    };
    api_error(StatusCode::FORBIDDEN, code, message)
}

/// Returns a 409 JSON error.
pub(crate) fn conflict(code: &str, msg: impl Into<String>) -> Response<Body> {
    api_error(StatusCode::CONFLICT, code, msg)
}

/// Returns a 404 JSON error.
pub(crate) fn not_found(msg: impl Into<String>) -> Response<Body> {
    let message = msg.into();
    let code = if message.contains("suitable engineer") {
        "NO_SUITABLE_ENGINEER"
    } else if message.contains("engineer") {
        "ENGINEER_NOT_FOUND"
    } else if message.contains("task") {
        "TASK_NOT_FOUND"
    } else {
        "NOT_FOUND"
    };
    api_error(StatusCode::NOT_FOUND, code, message)
}

/// JSON body that answers a parse failure with `ErrorResponse` and status 422.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = Response<Body>;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(err) => Err(unprocessable(err.body_text())),
        }
    }
}

/// Query string that answers a parse failure with status 422.
pub struct ApiQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    axum::extract::Query<T>: FromRequestParts<S>,
    S: Send + Sync,
    <axum::extract::Query<T> as FromRequestParts<S>>::Rejection: std::fmt::Display,
{
    type Rejection = Response<Body>;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(value)) => Ok(ApiQuery(value)),
            Err(err) => Err(unprocessable(err.to_string())),
        }
    }
}

pub(crate) fn resolve_page(query: PageQuery) -> Result<(usize, usize), Response<Body>> {
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);
    if !(1..=1000).contains(&limit) || offset < 0 {
        return Err(unprocessable(
            "limit must be between 1 and 1000 and offset must be >= 0",
        ));
    }
    Ok((limit as usize, offset as usize))
}

pub(crate) fn slice_page<T>(items: Vec<T>, limit: usize, offset: usize) -> (Vec<T>, u64) {
    let total = items.len() as u64;
    let page = items.into_iter().skip(offset).take(limit).collect();
    (page, total)
}

pub(crate) fn active_map(state: &AppState) -> Result<ActiveMap, Response<Body>> {
    state.active.read().map(|guard| guard.clone()).map_err(|e| {
        tracing::error!(error = %e, "map lock poisoned");
        internal_error()
    })
}

// ---------------------------------------------------------------------------
// Map
// ---------------------------------------------------------------------------

/// Returns the current airport map matrix.
#[utoipa::path(
    get,
    path = "/api/map",
    tag = "Map",
    operation_id = "getMap",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Current map matrix", body = MapResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
    ),
)]
pub async fn get_map(State(app_state): State<Arc<AppState>>) -> Response<Body> {
    let active = match active_map(&app_state) {
        Ok(active) => active,
        Err(response) => return response,
    };
    let document = active.document.clone();
    Json(MapResponse {
        id: active.id,
        name: active.name,
        matrix: active.map,
        format: document.format,
        width: document.width,
        height: document.height,
        cell_size_m: document.cell_size_m,
        origin: document.origin,
        palette: document.palette.clone(),
        layers: document.layers.clone(),
        objects: document.objects.clone(),
        zones: document.zones.clone(),
        overlays: document.overlays.clone(),
        updated_at: active.loaded_at.to_rfc3339(),
    })
    .into_response()
}

/// Lists every map file the server can switch to.
#[utoipa::path(
    get,
    path = "/api/maps",
    tag = "Map",
    operation_id = "listMaps",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Maps discovered next to the CLI map", body = MapListResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn list_maps(State(app_state): State<Arc<AppState>>) -> Response<Body> {
    let active = match active_map(&app_state) {
        Ok(active) => active,
        Err(response) => return response,
    };
    let maps = app_state
        .catalog
        .entries
        .iter()
        .map(|entry| MapSummary {
            active: entry.id == active.id,
            id: entry.id.clone(),
            name: entry.name.clone(),
            width: entry.width as i64,
            height: entry.height as i64,
        })
        .collect();
    Json(MapListResponse {
        maps,
        active_id: active.id,
    })
    .into_response()
}

/// Returns the id of the map routing and positions currently use.
#[utoipa::path(
    get,
    path = "/api/maps/active",
    tag = "Map",
    operation_id = "getActiveMap",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Active map", body = ActiveMapResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn get_active_map(State(app_state): State<Arc<AppState>>) -> Response<Body> {
    let active = match active_map(&app_state) {
        Ok(active) => active,
        Err(response) => return response,
    };
    Json(ActiveMapResponse {
        id: active.id,
        name: active.name,
        width: active.map.width() as i64,
        height: active.map.height() as i64,
        updated_at: active.loaded_at.to_rfc3339(),
    })
    .into_response()
}

/// Switches the map used by routing, positions and AOG.
#[utoipa::path(
    put,
    path = "/api/maps/active",
    tag = "Map",
    operation_id = "setActiveMap",
    description = "Dispatcher selects a map by id. Positions and AOG of the previous map stay in Redis under that map.",
    security(("bearerAuth" = [])),
    request_body = SelectMapRequest,
    responses(
        (status = 200, description = "Active map updated", body = ActiveMapResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 404, description = "Unknown map id", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn set_active_map(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<SelectMapRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can select the active map"));
    }
    if payload.id.trim().is_empty() {
        return Err(unprocessable("id is required"));
    }
    let entry = app_state
        .catalog
        .get(&payload.id)
        .cloned()
        .ok_or_else(|| not_found("map was not found"))?;
    let loaded = crate::maps::load_active(&entry).map_err(|e| {
        tracing::error!(error = %e, map_id = %entry.id, "failed to load selected map");
        internal_error()
    })?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;
    crate::maps::store_active(&mut redis_conn, &loaded.id).map_err(|e| {
        tracing::error!(error = %e, "failed to store the active map id");
        internal_error()
    })?;
    let response = ActiveMapResponse {
        id: loaded.id.clone(),
        name: loaded.name.clone(),
        width: loaded.map.width() as i64,
        height: loaded.map.height() as i64,
        updated_at: loaded.loaded_at.to_rfc3339(),
    };
    let mut guard = app_state.active.write().map_err(|e| {
        tracing::error!(error = %e, "map lock poisoned");
        internal_error()
    })?;
    *guard = loaded;
    Ok(Json(response).into_response())
}

// ---------------------------------------------------------------------------
// Route calculation
// ---------------------------------------------------------------------------

/// Computes the shortest path between two points on the map.
#[utoipa::path(
    post,
    path = "/api/getRoute",
    tag = "Route",
    operation_id = "getRoute",
    description = "Time-optimal route on the road graph, using per-edge speeds",
    security(("bearerAuth" = [])),
    request_body = GetRouteRequest,
    responses(
        (status = 200, description = "Successfully found a route", body = GetRouteResponse),
        (status = 400, description = "Invalid input data", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn get_route(
    State(app_state): State<Arc<AppState>>,
    ApiJson(payload): ApiJson<GetRouteRequest>,
) -> Result<Response<Body>, Response<Body>> {
    let active = active_map(&app_state)?;
    if !active.map.in_bounds(payload.start_point) || !active.map.in_bounds(payload.end_point) {
        return Err(bad_request("point is outside of map bounds"));
    }
    let agent = match payload.agent {
        crate::types::dto::RouteAgent::Engineer => crate::graph::AgentKind::Engineer,
        crate::types::dto::RouteAgent::Vehicle => crate::graph::AgentKind::Vehicle,
    };
    let planned = active
        .location
        .graph
        .route_stops(agent, &[payload.start_point, payload.end_point])
        .map_err(|error| bad_request(error.to_string()))?;
    let route = Route::new(planned.points);

    let distance = route.len();
    let eta_seconds = planned.seconds.ceil() as u64;
    Ok(Json(GetRouteResponse {
        route,
        distance,
        eta_seconds,
        agent: payload.agent,
    })
    .into_response())
}

// ---------------------------------------------------------------------------
// Current task
// ---------------------------------------------------------------------------

/// Fetches the currently active task for the authenticated engineer.
#[utoipa::path(
    get,
    path = "/api/tasks/current",
    tag = "Tasks",
    operation_id = "getCurrentTask",
    description = "Returns the engineer's active task. Returns null if no active task.",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Current active task", body = Option<CurrentTaskResponse>),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not an engineer", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn get_current_task(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Engineer {
        return Err(forbidden("only engineers can view their tasks"));
    }

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;

    let active = active_map(&app_state)?;
    use self::tasks::dsl::*;
    let task = tasks
        .filter(assigned_engineer.eq(auth_user.id))
        .filter(is_active.eq(true))
        .filter(map_id.is_null().or(map_id.eq(active.id.clone())))
        .order(created_at.desc())
        .select(Task::as_select())
        .first::<Task>(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load current task");
            internal_error()
        })?;

    let Some(task) = task else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let route: Route =
        serde_json::from_str(&task.task_route).unwrap_or_else(|_| Route::new(Vec::new()));
    let mut eta = task.eta_seconds.unwrap_or(0) as u64;
    if let Ok(mut redis_conn) = app_state.redis_pool.get() {
        if let Some(engineer_id) = task.assigned_engineer {
            if let Ok(Some(raw)) = redis_conn.get(active.engineer_position_key(engineer_id)) {
                if let Ok(from) = Point::from_value(raw) {
                    let target = Point::new(task.plane_x, task.plane_y);
                    if let Ok(planned) = active
                        .location
                        .graph
                        .route_stops(crate::graph::AgentKind::Engineer, &[from, target])
                    {
                        eta = planned.seconds.ceil() as u64;
                    }
                    let inside = active.location.zones.iter().any(|zone| {
                        active
                            .location
                            .zone_contains(zone, task.plane_x, task.plane_y)
                            && active.location.zone_contains(zone, from.0, from.1)
                    }) || {
                        let dx = (from.0 - task.plane_x) as f64 * active.location.cell_size_m;
                        let dy = (from.1 - task.plane_y) as f64 * active.location.cell_size_m;
                        dx * dx + dy * dy <= 12.0 * 12.0
                    };
                    if inside && task.arrived_at.is_none() {
                        let _ = diesel::update(tasks.find(task.id))
                            .set(arrived_at.eq(Some(Utc::now())))
                            .execute(&mut pg_conn);
                        crate::journal::record(
                            &mut pg_conn,
                            "arrived_at_aircraft",
                            Some(&auth_user.id.to_string()),
                            Some(task.id),
                            serde_json::json!({ "x": from.0, "y": from.1 }),
                            None,
                        );
                    }
                }
            }
        }
    }

    Ok(Json(Some(CurrentTaskResponse {
        id: task.id.to_string(),
        description: task.description,
        issue: task.issue_type,
        plane_point: Point::new(task.plane_x, task.plane_y),
        plane_uuid: task.plane_id.map(|aircraft_id| aircraft_id.to_string()),
        route,
        status: task.status,
        eta_seconds: eta,
        ata_chapter: task.ata_chapter,
        mel_category: task.mel_category,
        arrived: task.arrived_at.is_some(),
        tools_picked: task.tools_picked,
    }))
    .into_response())
}

/// Sets or clears the AOG status of an aircraft for the engineer's current task.
///
/// The status is stored in Redis under `aog:{plane_uuid}`.
#[utoipa::path(
    put,
    path = "/api/tasks/current/aog",
    tag = "Tasks",
    operation_id = "setCurrentTaskAog",
    description = "Sets or clears AOG on the aircraft of the engineer's current task. Always returns 200.",
    security(("bearerAuth" = [])),
    request_body = CurrentTaskAogRequest,
    responses(
        (status = 200, description = "AOG status updated", body = CurrentTaskAogResponse),
        (status = 400, description = "Current task has no aircraft", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not an engineer", body = ErrorResponse),
        (status = 404, description = "Engineer has no active task", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn current_task_aog(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<CurrentTaskAogRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Engineer {
        return Err(forbidden("only engineers can change AOG status"));
    }

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let active = active_map(&app_state)?;
    use self::tasks::dsl::*;
    let task = tasks
        .filter(assigned_engineer.eq(auth_user.id))
        .filter(is_active.eq(true))
        .filter(map_id.is_null().or(map_id.eq(active.id.clone())))
        .order(created_at.desc())
        .select(Task::as_select())
        .first::<Task>(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load current task");
            internal_error()
        })?;

    let Some(task) = task else {
        return Err(not_found("engineer has no active task"));
    };
    let Some(plane_uuid) = task.plane_id else {
        return Err(bad_request("current task has no aircraft"));
    };

    let aog = payload
        .aog
        .or(payload.toggle)
        .ok_or_else(|| unprocessable("aog is required"))?;
    let key = active.aog_key(plane_uuid);
    if aog {
        redis_conn.set(&key, "1").map_err(|e| {
            tracing::error!(%plane_uuid, error = %e, "failed to set aircraft AOG status");
            internal_error()
        })?;
    } else {
        redis_conn.del(&key).map_err(|e| {
            tracing::error!(%plane_uuid, error = %e, "failed to clear aircraft AOG status");
            internal_error()
        })?;
    }

    Ok(Json(CurrentTaskAogResponse {
        plane_uuid: plane_uuid.to_string(),
        aog,
    })
    .into_response())
}

/// Previous POST form of [`current_task_aog`]. Same handler, kept so existing clients keep working.
#[utoipa::path(
    post,
    path = "/api/tasks/current/aog",
    tag = "Tasks",
    operation_id = "setCurrentTaskAogLegacy",
    description = "Deprecated alias of PUT /api/tasks/current/aog. Always returns 200.",
    security(("bearerAuth" = [])),
    request_body = CurrentTaskAogRequest,
    responses(
        (status = 200, description = "AOG status updated", body = CurrentTaskAogResponse),
        (status = 400, description = "Current task has no aircraft", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not an engineer", body = ErrorResponse),
        (status = 404, description = "Engineer has no active task", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn current_task_aog_post(
    state: State<Arc<AppState>>,
    auth_user: Extension<AuthUser>,
    payload: ApiJson<CurrentTaskAogRequest>,
) -> Result<Response<Body>, Response<Body>> {
    current_task_aog(state, auth_user, payload).await
}

/// Lists aircraft UUIDs that currently have an AOG status.
#[utoipa::path(
    get,
    path = "/api/tasks/aog",
    tag = "Aircraft",
    operation_id = "listAog",
    security(("bearerAuth" = [])),
    params(PageQuery),
    responses(
        (status = 200, description = "Aircraft currently marked AOG", body = AogListResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 422, description = "Query parameters do not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn list_aog(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiQuery(page): ApiQuery<PageQuery>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can view AOG aircraft"));
    }

    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let (limit, offset) = resolve_page(page)?;
    let active = active_map(&app_state)?;
    let mut aircraft = load_aog_aircraft(&mut redis_conn, &active)?;
    attach_arrival_times(&mut pg_conn, &active.id, &mut aircraft)?;
    let plane_uuids: Vec<String> = aircraft.iter().map(|entry| entry.id.to_string()).collect();
    let (aircraft, total) = slice_page(aircraft, limit, offset);
    let plane_uuids = {
        let (_, _) = (limit, offset);
        let start = offset.min(plane_uuids.len());
        let end = start.saturating_add(limit).min(plane_uuids.len());
        plane_uuids[start..end].to_vec()
    };
    Ok(Json(AogListResponse {
        aircraft,
        total,
        plane_uuids,
    })
    .into_response())
}

/// Records an aircraft arrival and places it on the active map.
#[utoipa::path(
    post,
    path = "/api/aircraft/arrive",
    tag = "Aircraft",
    operation_id = "arriveAircraft",
    description = "Adds an aircraft to the active map and stores the arrival time.",
    security(("bearerAuth" = [])),
    request_body = ArriveAircraftRequest,
    responses(
        (status = 200, description = "Aircraft arrived", body = AircraftMovementResponse),
        (status = 400, description = "Invalid UUID or position outside the map", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 409, description = "Aircraft is already on the active map", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn arrive_aircraft(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<ArriveAircraftRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden(
            "only dispatchers can register an aircraft arrival",
        ));
    }

    let aircraft_id = match payload.id.as_deref() {
        None | Some("") => Uuid::new_v4(),
        Some(raw) => Uuid::from_str(raw).map_err(|_| bad_request("invalid aircraft UUID"))?,
    };
    let arrived_at = parse_movement_time(payload.arrived_at)?;
    let active = active_map(&app_state)?;
    if !active.map.in_bounds(payload.point) {
        return Err(bad_request("aircraft position is outside of map bounds"));
    }

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let cell_budy = aircraft_movements::table
        .filter(aircraft_movements::map_id.eq(&active.id))
        .filter(aircraft_movements::departed_at.is_null())
        .filter(aircraft_movements::plane_x.eq(&payload.point.0))
        .filter(aircraft_movements::plane_y.eq(&payload.point.1))
        .select(aircraft_movements::id)
        .first::<Uuid>(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to look up an existing plane in cell");
            internal_error()
        })?;

    if cell_budy.is_some() {
        return Err(conflict(
            "CELL_IS_BUSY",
            "there's another plane at this cell",
        ));
    }

    let already_open = aircraft_movements::table
        .filter(aircraft_movements::aircraft_id.eq(aircraft_id))
        .filter(aircraft_movements::map_id.eq(&active.id))
        .filter(aircraft_movements::departed_at.is_null())
        .select(aircraft_movements::id)
        .first::<Uuid>(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to look up an open aircraft movement");
            internal_error()
        })?;
    if already_open.is_some() {
        return Err(conflict(
            "AIRCRAFT_ALREADY_ACTIVE",
            "aircraft is already on the active map",
        ));
    }

    let movement = AircraftMovement {
        id: Uuid::new_v4(),
        aircraft_id,
        map_id: active.id.clone(),
        plane_x: payload.point.0,
        plane_y: payload.point.1,
        arrived_at,
        departed_at: None,
    };
    diesel::insert_into(aircraft_movements::table)
        .values(&movement)
        .execute(&mut pg_conn)
        .map_err(|e| {
            if matches!(
                e,
                diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    _
                )
            ) {
                return conflict(
                    "AIRCRAFT_ALREADY_ACTIVE",
                    "aircraft is already on the active map",
                );
            }
            tracing::error!(error = %e, "failed to store an aircraft arrival");
            internal_error()
        })?;

    if let Err(error) = redis_conn.set(
        active.vehicle_position_key(&VehicleType::Aircraft, aircraft_id),
        payload.point.as_value(),
    ) {
        tracing::error!(error = %error, "failed to place an arrived aircraft");
        let _ = diesel::delete(aircraft_movements::table.find(movement.id)).execute(&mut pg_conn);
        return Err(internal_error());
    }

    crate::journal::record(
        &mut pg_conn,
        "aircraft_arrived",
        Some(&auth_user.id.to_string()),
        None,
        serde_json::json!({
            "aircraft_id": aircraft_id,
            "arrived_at": arrived_at.to_rfc3339(),
            "point": [payload.point.0, payload.point.1]
        }),
        None,
    );
    Ok(Json(movement_response(&movement)).into_response())
}

/// Records an aircraft departure and removes it from the active map.
#[utoipa::path(
    post,
    path = "/api/aircraft/depart",
    tag = "Aircraft",
    operation_id = "departAircraft",
    description = "Removes an aircraft from the active map and stores the departure time. Clears its AOG flag.",
    security(("bearerAuth" = [])),
    request_body = DepartAircraftRequest,
    responses(
        (status = 200, description = "Aircraft departed", body = AircraftMovementResponse),
        (status = 400, description = "Departure is earlier than arrival", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 404, description = "Aircraft is not on the active map", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn depart_aircraft(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<DepartAircraftRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden(
            "only dispatchers can register an aircraft departure",
        ));
    }
    let aircraft_id =
        Uuid::from_str(&payload.id).map_err(|_| bad_request("invalid aircraft UUID"))?;
    let departed_at = parse_movement_time(payload.departed_at)?;
    let active = active_map(&app_state)?;

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let movement = aircraft_movements::table
        .filter(aircraft_movements::aircraft_id.eq(aircraft_id))
        .filter(aircraft_movements::map_id.eq(&active.id))
        .filter(aircraft_movements::departed_at.is_null())
        .select(AircraftMovement::as_select())
        .first::<AircraftMovement>(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load an open aircraft movement");
            internal_error()
        })?;
    let Some(movement) = movement else {
        return Err(not_found("aircraft was not found"));
    };
    if departed_at < movement.arrived_at {
        return Err(bad_request("departure is earlier than arrival"));
    }

    let position_key = active.vehicle_position_key(&VehicleType::Aircraft, aircraft_id);
    let previous_position: Option<String> = redis_conn.get(&position_key).map_err(|e| {
        tracing::error!(error = %e, "failed to read an aircraft position");
        internal_error()
    })?;
    redis_conn.del(&position_key).map_err(|e| {
        tracing::error!(error = %e, "failed to remove a departed aircraft");
        internal_error()
    })?;
    redis_conn.del(active.aog_key(aircraft_id)).map_err(|e| {
        tracing::error!(error = %e, "failed to clear AOG of a departed aircraft");
        internal_error()
    })?;

    let closed = AircraftMovement {
        departed_at: Some(departed_at),
        ..movement
    };
    if let Err(error) = diesel::update(aircraft_movements::table.find(closed.id))
        .set(aircraft_movements::departed_at.eq(closed.departed_at))
        .execute(&mut pg_conn)
    {
        tracing::error!(error = %error, "failed to store an aircraft departure");
        if let Some(position) = previous_position {
            let _ = redis_conn.set(&position_key, position);
        }
        return Err(internal_error());
    }

    crate::journal::record(
        &mut pg_conn,
        "aircraft_departed",
        Some(&auth_user.id.to_string()),
        None,
        serde_json::json!({
            "aircraft_id": aircraft_id,
            "departed_at": departed_at.to_rfc3339()
        }),
        None,
    );
    Ok(Json(movement_response(&closed)).into_response())
}

/// Lists arrival and departure records of the active map.
#[utoipa::path(
    get,
    path = "/api/aircraft/movements",
    tag = "Aircraft",
    operation_id = "listAircraftMovements",
    description = "Arrival and departure times recorded for the active map, newest arrival first.",
    security(("bearerAuth" = [])),
    params(PageQuery),
    responses(
        (status = 200, description = "Aircraft movements", body = AircraftMovementListResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 422, description = "Query parameters do not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn list_aircraft_movements(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiQuery(page): ApiQuery<PageQuery>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can view aircraft movements"));
    }
    let active = active_map(&app_state)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let (limit, offset) = resolve_page(page)?;
    let rows = aircraft_movements::table
        .filter(aircraft_movements::map_id.eq(&active.id))
        .order(aircraft_movements::arrived_at.desc())
        .select(AircraftMovement::as_select())
        .load::<AircraftMovement>(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to list aircraft movements");
            internal_error()
        })?;
    let total = rows.len() as u64;
    let movements = rows
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|row| movement_response(&row))
        .collect();
    Ok(Json(AircraftMovementListResponse { movements, total }).into_response())
}

/// Lists every aircraft currently stored in Redis.
#[utoipa::path(
    get,
    path = "/api/aircraft",
    tag = "Aircraft",
    operation_id = "listAircraft",
    security(("bearerAuth" = [])),
    params(PageQuery),
    responses(
        (status = 200, description = "Current aircraft", body = AircraftListResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 422, description = "Query parameters do not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn list_aircraft(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiQuery(page): ApiQuery<PageQuery>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can view aircraft"));
    }

    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let active = active_map(&app_state)?;
    let (limit, offset) = resolve_page(page)?;
    let mut aircraft = load_aircraft(&mut redis_conn, &active)?;
    attach_arrival_times(&mut pg_conn, &active.id, &mut aircraft)?;
    let (aircraft, total) = slice_page(aircraft, limit, offset);
    Ok(Json(AircraftListResponse { aircraft, total }).into_response())
}

/// Completes the current active task of the authenticated engineer.
#[utoipa::path(
    post,
    path = "/api/tasks/current/complete",
    tag = "Tasks",
    operation_id = "completeCurrentTask",
    summary = "Completes the current active task of the authenticated engineer",
    description = "Marks the current active task of the authenticated engineer as completed",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Task marked as completed"),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not an engineer", body = ErrorResponse),
        (status = 404, description = "Engineer has no active task", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn complete_current_task(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Engineer {
        return Err(forbidden("only engineers can view their tasks"));
    }

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;

    let active = active_map(&app_state)?;
    use self::tasks::dsl::*;
    let task = tasks
        .filter(assigned_engineer.eq(auth_user.id))
        .filter(is_active.eq(true))
        .filter(map_id.is_null().or(map_id.eq(active.id.clone())))
        .order(created_at.desc())
        .select(Task::as_select())
        .first::<Task>(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load current task");
            internal_error()
        })?;

    let Some(task) = task else {
        return Err(not_found("engineer has no active task"));
    };

    diesel::update(tasks.find(task.id))
        .set((is_active.eq(false), status.eq("done")))
        .execute(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to update task");
            internal_error()
        })?;
    crate::journal::record(
        &mut pg_conn,
        "status_changed",
        Some(&auth_user.id.to_string()),
        Some(task.id),
        serde_json::json!({ "from": task.status, "to": "done" }),
        None,
    );

    Ok(StatusCode::OK.into_response())
}

// ---------------------------------------------------------------------------
// Problem classification
// ---------------------------------------------------------------------------

/// Classifies a free-form problem description into an engineer specialization.
#[utoipa::path(
    post,
    path = "/api/classify",
    tag = "Classifier",
    operation_id = "classifyProblem",
    description = "Classify a problem description with TF-IDF weighted Naive Bayes",
    security(("bearerAuth" = [])),
    request_body = ClassifyRequest,
    responses(
        (status = 200, description = "Predicted engineer role", body = ClassifyResponse),
        (status = 400, description = "Description has no words", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn classify_problem(
    State(app_state): State<Arc<AppState>>,
    ApiJson(payload): ApiJson<ClassifyRequest>,
) -> Result<Response<Body>, Response<Body>> {
    let model = app_state.classifier.read().map_err(|e| {
        tracing::error!(error = %e, "classifier lock poisoned");
        internal_error()
    })?;
    let classification = model
        .classify(&payload.text)
        .map_err(|e| bad_request(e.to_string()))?;

    Ok(Json(ClassifyResponse {
        engineer_type: classification.engineer_type,
        confidence: classification.confidence,
    })
    .into_response())
}

/// Retrains the problem classifier from the TSV dataset configured at startup.
#[utoipa::path(
    post,
    path = "/api/classify/train",
    tag = "Classifier",
    operation_id = "trainClassifier",
    description = "Retrain the problem classifier from the dataset file",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Classifier retrained", body = TrainClassifierResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 500, description = "Dataset could not be loaded", body = ErrorResponse),
    ),
)]
pub async fn train_classifier(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can retrain the classifier"));
    }

    let trained =
        crate::types::classifier::ProblemClassifier::train_from_dataset(&app_state.dataset_path)
            .map_err(|e| {
                tracing::error!(error = %e, "failed to retrain problem classifier");
                internal_error()
            })?;
    let documents = trained.learned();

    let mut model = app_state.classifier.write().map_err(|e| {
        tracing::error!(error = %e, "classifier lock poisoned");
        internal_error()
    })?;
    *model = trained;

    Ok(Json(TrainClassifierResponse {
        documents,
        trained_at: Utc::now().to_rfc3339(),
        accuracy: None,
    })
    .into_response())
}

// ---------------------------------------------------------------------------
// Engineer assignment
// ---------------------------------------------------------------------------

/// Creates a new task and assigns the most suitable engineer.
#[utoipa::path(
    post,
    path = "/api/assign",
    tag = "Assign",
    operation_id = "assignEngineer",
    description = "Classify the problem text, then find the nearest suitable engineer and optionally pick up a special vehicle",
    security(("bearerAuth" = [])),
    request_body = AssignEngineerRequest,
    responses(
        (status = 200, description = "Task created, engineer assigned", body = AssignEngineerResponse),
        (status = 400, description = "Invalid input or route error", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 404, description = "No suitable engineer or transport found", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn assign_engineer(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<AssignEngineerRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can assign engineers"));
    }

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let task = load_request_for_assignment(&mut pg_conn, payload.request_id.as_deref(), true)?;
    let plan = build_candidates(&app_state, &mut pg_conn, &mut redis_conn, &task).await?;
    let candidates: Vec<AssignmentCandidate> = plan
        .outcome
        .candidates
        .iter()
        .map(assignment_candidate)
        .collect();
    crate::journal::record(
        &mut pg_conn,
        "candidates_built",
        Some(&auth_user.id.to_string()),
        Some(task.id),
        serde_json::json!({ "count": candidates.len(), "candidates": candidates }),
        Some(plan.outcome.elapsed_ms),
    );

    // A named engineer is assigned as told; without one the best eligible row
    // wins. Neither path asks whether the engineer is the nearest.
    let chosen = match payload
        .engineer_id
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
    {
        None => plan
            .outcome
            .candidates
            .iter()
            .find(|row| row.eligible)
            .ok_or_else(|| not_found("no suitable engineer available"))?,
        Some(raw) => {
            let engineer_id =
                Uuid::from_str(raw).map_err(|_| bad_request("invalid engineer id"))?;
            let row = plan
                .outcome
                .candidates
                .iter()
                .find(|row| row.engineer_id == engineer_id)
                .ok_or_else(|| bad_request("engineer was not found"))?;
            if !row.eligible {
                return Err(bad_request(format!(
                    "engineer cannot take this request: {}",
                    row.reason
                )));
            }
            row
        }
    };

    let travel = chosen.eta_seconds;
    let sla_exceeded = travel >= plan.active.location.sla_secs;
    let route_json = serde_json::to_string(&chosen.route).expect("route serializes");
    // ТЗ п.17: the numbers behind the pick travel with the task, so the
    // engineer can be shown why they were chosen when they accept it.
    let basis = SelectionBasis {
        engineer_id: chosen.engineer_id.to_string(),
        score: chosen.score,
        eta_seconds: travel,
        distance: chosen.route.len() as u64,
        uses_vehicle: chosen.uses_vehicle,
        factors: chosen
            .factors
            .iter()
            .map(|factor| ScoreFactor {
                name: factor.name.clone(),
                seconds: factor.seconds,
            })
            .collect(),
        reason: chosen.reason.clone(),
        manual: payload.engineer_id.is_some(),
        chosen_at: Utc::now().to_rfc3339(),
    };
    let basis_json = serde_json::to_string(&basis).map_err(|e| {
        tracing::error!(error = %e, "failed to serialize the selection basis");
        internal_error()
    })?;
    diesel::update(tasks::table.find(task.id))
        .set((
            tasks::assigned_engineer.eq(Some(chosen.engineer_id)),
            tasks::status.eq("assigned"),
            tasks::task_route.eq(&route_json),
            tasks::eta_seconds.eq(Some(travel as i32)),
            tasks::candidate_ms.eq(Some(plan.outcome.elapsed_ms)),
            tasks::selection_basis.eq(Some(basis_json)),
            tasks::ata_chapter.eq(&plan.chapter),
            tasks::ends_at
                .eq(task.recorded_at
                    + chrono::Duration::seconds(plan.active.location.sla_secs as i64)),
            tasks::is_active.eq(true),
            tasks::waiting_part.eq(false),
            tasks::arrived_at.eq(None::<DateTime<Utc>>),
            tasks::tools_picked.eq(false),
        ))
        .execute(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to assign request");
            internal_error()
        })?;
    crate::journal::record(
        &mut pg_conn,
        "engineer_assigned",
        Some(&auth_user.id.to_string()),
        Some(task.id),
        serde_json::json!({
            "engineer_id": chosen.engineer_id,
            "eta_seconds": travel,
            "uses_vehicle": chosen.uses_vehicle,
            "reason": chosen.reason,
            "manual": payload.engineer_id.is_some(),
        }),
        None,
    );
    crate::journal::record(
        &mut pg_conn,
        "route_built",
        Some(&auth_user.id.to_string()),
        Some(task.id),
        serde_json::json!({ "points": chosen.route.len(), "eta_seconds": travel }),
        None,
    );

    Ok(Json(AssignEngineerResponse {
        engineer_uuid: chosen.engineer_id.to_string(),
        eta_seconds: travel,
        sla_exceeded,
        distance: chosen.route.len() as u64,
        time: travel,
        time_limit_exceeded: sla_exceeded,
        candidates,
        candidate_ms: plan.outcome.elapsed_ms,
        route: chosen.route.clone(),
    })
    .into_response())
}

/// Ranks the engineers for one request without changing it: the dispatcher
/// reads this before deciding who to assign, and nothing here writes a row.
#[utoipa::path(
    post,
    path = "/api/assign/candidates",
    tag = "Assign",
    operation_id = "assignCandidates",
    description = "Ranks every engineer for a request. Read-only: the task is not modified.",
    security(("bearerAuth" = [])),
    request_body = AssignCandidatesRequest,
    responses(
        (status = 200, description = "Ranked candidates", body = AssignmentCandidatesResponse),
        (status = 400, description = "Invalid input or route error", body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 404, description = "Request was not found", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
        (status = 500, description = "Server-side error", body = ErrorResponse),
    ),
)]
pub async fn assign_candidates(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<AssignCandidatesRequest>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can rank engineers"));
    }

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let task = load_request_for_assignment(&mut pg_conn, Some(&payload.request_id), false)?;
    let plan = build_candidates(&app_state, &mut pg_conn, &mut redis_conn, &task).await?;

    Ok(Json(AssignmentCandidatesResponse {
        request_id: task.id.to_string(),
        chapter: plan.chapter.clone(),
        candidates: plan
            .outcome
            .candidates
            .iter()
            .map(assignment_candidate)
            .collect(),
        candidate_ms: plan.outcome.elapsed_ms,
    })
    .into_response())
}

// ---------------------------------------------------------------------------
// assign_engineer helpers
// ---------------------------------------------------------------------------

/// What [`build_candidates`] produced: the ranking, the ATA chapter it ranked
/// for and the map it ranked on. Shared by the preview and the assignment.
pub(crate) struct AssignmentPlan {
    pub outcome: crate::rank::RankOutcome,
    pub chapter: Option<String>,
    pub active: ActiveMap,
}

/// One published row of the ranking. Both handlers must publish the same
/// fields, so the mapping lives here once.
pub(crate) fn assignment_candidate(row: &crate::rank::CandidateScore) -> AssignmentCandidate {
    AssignmentCandidate {
        engineer_id: row.engineer_id.to_string(),
        eligible: row.eligible,
        reason: row.reason.clone(),
        eta_seconds: row.eta_seconds,
        uses_vehicle: row.uses_vehicle,
        late_seconds: row.components.late,
        load_penalty: row.components.load,
        aog_bonus: row.components.aog_bonus,
        arrival_bonus: row.components.arrival_bonus,
        urgency_bonus: row.components.urgency_bonus,
        mel_bonus: row.components.mel_bonus,
        factors: row
            .factors
            .iter()
            .map(|factor| ScoreFactor {
                name: factor.name.clone(),
                seconds: factor.seconds,
            })
            .collect(),
        score: row.score,
    }
}

/// Loads the request the dispatcher is looking at. Assignment needs it in the
/// pool; the preview also serves tasks that already hold an engineer, because
/// that is where the dispatcher picks the one to move them to.
fn load_request_for_assignment(
    pg_conn: &mut diesel::PgConnection,
    request_id: Option<&str>,
    pool_only: bool,
) -> Result<Task, Response<Body>> {
    let request_id = request_id
        .filter(|raw| !raw.is_empty())
        .ok_or_else(|| unprocessable("request_id is required"))?;
    let request_id = Uuid::from_str(request_id).map_err(|_| bad_request("invalid request id"))?;
    let task = tasks::table
        .find(request_id)
        .select(Task::as_select())
        .first::<Task>(pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load request");
            internal_error()
        })?;
    let Some(task) = task else {
        return Err(not_found("request was not found"));
    };
    if is_terminal_status(&task.status) {
        return Err(bad_request(format!(
            "a request in status {} has no engineers left to rank",
            task.status
        )));
    }
    if pool_only && task.status != "unassigned" {
        return Err(bad_request("request is not waiting for assignment"));
    }
    Ok(task)
}

/// A finished job is nobody's to move: `done` and `rejected` are the end.
pub(crate) fn is_terminal_status(status: &str) -> bool {
    matches!(status, "done" | "rejected")
}

/// Ranks every engineer for `task`. Reads only: the classification hint, the
/// engineer roster and positions, the open-task load and the transport
/// positions — no `tasks` row is touched.
pub(crate) async fn build_candidates(
    app_state: &AppState,
    pg_conn: &mut diesel::PgConnection,
    redis_conn: &mut redis::Connection,
    task: &Task,
) -> Result<AssignmentPlan, Response<Body>> {
    let active = active_map(app_state)?;
    let plane_point = Point::new(task.plane_x, task.plane_y);
    if !active.map.in_bounds(plane_point) {
        return Err(bad_request("aircraft position is outside of map bounds"));
    }

    let hint = {
        let model = app_state.classifier.read().map_err(|e| {
            tracing::error!(error = %e, "classifier lock poisoned");
            internal_error()
        })?;
        model
            .classify(&task.issue_type)
            .ok()
            .map(|item| item.engineer_type)
    };
    let chapter = task.ata_chapter.clone().or_else(|| {
        hint.and_then(|kind| {
            let name = crate::rank::engineer_type_name(kind);
            active
                .location
                .ata
                .iter()
                .find(|item| item.admissions.iter().any(|admission| admission == name))
                .map(|item| item.chapter.clone())
        })
    });

    let roster: Vec<(Uuid, crate::types::enums::EngineerType)> = engineers::table
        .select((engineers::id, engineers::engineer_type))
        .load(pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load engineers");
            internal_error()
        })?;
    let ids: Vec<Uuid> = roster.iter().map(|(id, _)| *id).collect();
    let by_point = load_engineer_positions(redis_conn, &active, &ids)?;
    let mut positions = HashMap::new();
    for (point, id) in by_point {
        positions.insert(id, point);
    }

    let open_rows: Vec<(Option<Uuid>, String, Option<Uuid>)> = tasks::table
        .filter(tasks::is_active.eq(true))
        .filter(tasks::id.ne(task.id))
        .filter(tasks::map_id.is_null().or(tasks::map_id.eq(&active.id)))
        .select((tasks::assigned_engineer, tasks::status, tasks::plane_id))
        .load(pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load open requests");
            internal_error()
        })?;
    let mut in_progress = HashMap::new();
    let mut load = HashMap::new();
    for (engineer, state, _plane) in open_rows {
        let Some(engineer) = engineer else { continue };
        if state == "in_progress" {
            in_progress.insert(engineer, true);
            *load.entry(engineer).or_insert(0) += 1;
        } else if state == "assigned" {
            *load.entry(engineer).or_insert(0) += 1;
        }
        // `waiting_part` is no longer special: the ТЗ requires that engineer to
        // be back in the common pool, so it only counts towards the workload.
        else if state == "waiting_part" {
            *load.entry(engineer).or_insert(0) += 1;
        }
    }

    let aircraft = match task.plane_id {
        Some(plane_uuid) => {
            crate::aircraft_context::aircraft_context(pg_conn, redis_conn, &active, plane_uuid)
                .map_err(|e| {
                    tracing::error!(error = %e, "failed to load aircraft context");
                    internal_error()
                })?
        }
        None => crate::aircraft_context::AircraftContext::default(),
    };

    let mut transports = HashMap::new();
    for vehicle_type in [
        VehicleType::FuelTruck,
        VehicleType::OilCart,
        VehicleType::MaintenanceLift,
        VehicleType::BorescopeCart,
        VehicleType::AvionicsTestSet,
    ] {
        transports.extend(load_transport_positions(redis_conn, &active, vehicle_type)?);
    }

    let outcome = crate::rank::rank(crate::rank::RankInput {
        map: &active,
        plane: plane_point,
        chapter: chapter.as_deref(),
        aircraft,
        engineers: roster,
        positions,
        in_progress,
        load,
        urgency: task.urgency.as_str(),
        mel_category: task.mel_category.as_deref(),
        transports,
        needs_tools: !task.tools_picked && active.location.object_point("warehouse").is_some(),
    });
    Ok(AssignmentPlan {
        outcome,
        chapter,
        active,
    })
}

// ---------------------------------------------------------------------------
// assign_engineer helpers
// ---------------------------------------------------------------------------

/// Loads the current map position of an aircraft (`position:aircraft:{uuid}`).
fn parse_movement_time(raw: Option<String>) -> Result<DateTime<Utc>, Response<Body>> {
    match raw {
        None => Ok(Utc::now()),
        Some(text) => DateTime::parse_from_rfc3339(&text)
            .map(|time| time.with_timezone(&Utc))
            .map_err(|_| unprocessable("time must be RFC3339")),
    }
}

fn movement_response(movement: &AircraftMovement) -> AircraftMovementResponse {
    AircraftMovementResponse {
        id: movement.id,
        aircraft_id: movement.aircraft_id,
        map_id: movement.map_id.clone(),
        point: Point::new(movement.plane_x, movement.plane_y),
        arrived_at: movement.arrived_at.to_rfc3339(),
        departed_at: movement.departed_at.map(|time| time.to_rfc3339()),
    }
}

fn attach_arrival_times(
    pg_conn: &mut diesel::PgConnection,
    map_id: &str,
    aircraft: &mut [AircraftEntry],
) -> Result<(), Response<Body>> {
    if aircraft.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = aircraft.iter().map(|entry| entry.id).collect();
    let rows: Vec<(Uuid, DateTime<Utc>)> = aircraft_movements::table
        .filter(aircraft_movements::map_id.eq(map_id))
        .filter(aircraft_movements::departed_at.is_null())
        .filter(aircraft_movements::aircraft_id.eq_any(&ids))
        .select((
            aircraft_movements::aircraft_id,
            aircraft_movements::arrived_at,
        ))
        .load(pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load aircraft arrival times");
            internal_error()
        })?;
    let by_id: HashMap<Uuid, DateTime<Utc>> = rows.into_iter().collect();
    for entry in aircraft {
        entry.arrived_at = by_id.get(&entry.id).map(|time| time.to_rfc3339());
    }
    Ok(())
}

/// Loads every aircraft stored as a vehicle in Redis.
fn load_aircraft(
    redis_conn: &mut redis::Connection,
    map: &ActiveMap,
) -> Result<Vec<AircraftEntry>, Response<Body>> {
    let keys: Vec<String> = redis_conn
        .scan_match(map.vehicle_scan("aircraft"))
        .map_err(|e| {
            tracing::error!(error = %e, "failed to scan aircraft keys");
            internal_error()
        })?
        .filter_map(Result::ok)
        .collect();

    let mut aircraft = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(uuid_str) = map.aircraft_id_from_key(&key) else {
            continue;
        };
        let id = match Uuid::from_str(uuid_str) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(%key, error = %e, "malformed aircraft key, skipped");
                continue;
            }
        };
        let raw = match redis_conn.get(&key) {
            Ok(Some(raw)) => raw,
            Ok(None) => continue,
            Err(e) => {
                tracing::error!(%key, error = %e, "failed to fetch aircraft position");
                return Err(internal_error());
            }
        };
        let point = match Point::from_value(raw) {
            Ok(point) => point,
            Err(e) => {
                tracing::warn!(%key, error = %e, "malformed aircraft position, skipped");
                continue;
            }
        };
        aircraft.push(AircraftEntry {
            id,
            point,
            arrived_at: None,
        });
    }

    aircraft.sort_by_key(|entry| entry.id);
    Ok(aircraft)
}

/// Loads aircraft that currently have an AOG flag (`aog:{uuid}`) and a known position.
fn load_aog_aircraft(
    redis_conn: &mut redis::Connection,
    map: &ActiveMap,
) -> Result<Vec<AircraftEntry>, Response<Body>> {
    let keys: Vec<String> = redis_conn
        .scan_match(map.aog_scan())
        .map_err(|e| {
            tracing::error!(error = %e, "failed to scan AOG keys");
            internal_error()
        })?
        .filter_map(Result::ok)
        .collect();

    let mut aircraft = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(uuid_str) = map.aog_id_from_key(&key) else {
            continue;
        };
        let id = match Uuid::from_str(uuid_str) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(%key, error = %e, "malformed AOG key, skipped");
                continue;
            }
        };
        let raw = match redis_conn.get(map.vehicle_position_key(&VehicleType::Aircraft, id)) {
            Ok(Some(raw)) => raw,
            Ok(None) => continue,
            Err(e) => {
                tracing::error!(%id, error = %e, "failed to fetch AOG aircraft position");
                return Err(internal_error());
            }
        };
        let point = match Point::from_value(raw) {
            Ok(point) => point,
            Err(e) => {
                tracing::warn!(%id, error = %e, "malformed AOG aircraft position, skipped");
                continue;
            }
        };
        aircraft.push(AircraftEntry {
            id,
            point,
            arrived_at: None,
        });
    }

    aircraft.sort_by_key(|entry| entry.id);
    Ok(aircraft)
}

/// Loads current engineer positions from Redis into a `Point -> Uuid` map.
fn load_engineer_positions(
    redis_conn: &mut redis::Connection,
    map: &ActiveMap,
    ids: &[Uuid],
) -> Result<HashMap<Point, Uuid>, Response<Body>> {
    let mut positions = HashMap::with_capacity(ids.len());

    for &id in ids {
        match redis_conn.get(map.engineer_position_key(id)) {
            Ok(Some(raw)) => match Point::from_value(raw) {
                Ok(point) => {
                    positions.insert(point, id);
                }
                Err(e) => {
                    tracing::warn!(%id, error = %e, "engineer position could not be parsed");
                }
            },
            Ok(None) => {
                tracing::warn!(%id, "engineer position is missing in redis");
            }
            Err(e) => {
                tracing::error!(%id, error = %e, "failed to fetch engineer position");
                return Err(internal_error());
            }
        }
    }

    Ok(positions)
}

/// Loads special-vehicle positions of the given type into a `Point -> Uuid` map.
fn load_transport_positions(
    redis_conn: &mut redis::Connection,
    map: &ActiveMap,
    vehicle_type: VehicleType,
) -> Result<HashMap<Point, Uuid>, Response<Body>> {
    let mut positions = HashMap::new();

    let vehicle_name = vehicle_type.to_string();
    let keys: Vec<String> = redis_conn
        .scan_match(map.vehicle_scan(&vehicle_name))
        .map_err(|e| {
            tracing::error!(error = %e, "failed to scan transport keys");
            internal_error()
        })?
        .filter_map(Result::ok)
        .collect();

    for key in keys {
        let value = match redis_conn.get(&key) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(e) => {
                tracing::error!(%key, error = %e, "failed to fetch transport position");
                return Err(internal_error());
            }
        };

        let uuid_str = match key.rsplit(':').next() {
            Some(s) => s,
            None => continue,
        };
        let uuid = match Uuid::from_str(uuid_str) {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!(%key, error = %e, "malformed transport uuid, skipped");
                continue;
            }
        };
        let point = match Point::from_value(value) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(%key, error = %e, "malformed transport position, skipped");
                continue;
            }
        };

        positions.insert(point, uuid);
    }

    Ok(positions)
}
