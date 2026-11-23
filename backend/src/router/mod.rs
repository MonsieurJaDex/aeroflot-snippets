//! HTTP router: common helpers, state and public handler definitions.

pub mod auth;
pub mod geo;

use std::{collections::HashMap, str::FromStr, sync::Arc, time::Duration};

use axum::extract::rejection::JsonRejection;
use axum::{
    Extension, Json,
    body::Body,
    extract::{FromRequest, FromRequestParts, Request, State},
    http::{Response, StatusCode, request::Parts},
    response::IntoResponse,
};

use chrono::Utc;
use diesel::prelude::*;
use redis::TypedCommands;
use uuid::Uuid;

use crate::{
    database::schema::{self, engineers, tasks},
    middleware::AuthUser,
    models::task::Task,
    maps::ActiveMap,
    search::find_nearest,
    types::{
        config::AppState,
        dto::{
            ActiveMapResponse, AircraftEntry, AircraftListResponse, AogListResponse,
            AssignEngineerRequest, AssignEngineerResponse, ClassifyRequest, ClassifyResponse,
            CurrentTaskAogRequest, CurrentTaskAogResponse, CurrentTaskResponse, ErrorResponse,
            GetRouteRequest, GetRouteResponse, MapListResponse, MapResponse, MapSummary, PageQuery,
            SelectMapRequest, TrainClassifierResponse,
        },
        enums::{UserRole, VehicleType},
        map::{Point, Route},
    },
};

// ---------------------------------------------------------------------------
// Route constants
// ---------------------------------------------------------------------------

/// Engineer walking speed, in map cells per second.
const ENGINEER_SPEED_CELLS_PER_SEC: f32 = 0.05;

/// Maximum acceptable response time (15 min), in seconds.
const SLA_LIMIT_SECS: f32 = 900.0;

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
    api_error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "VALIDATION_ERROR",
        msg,
    )
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

fn active_map(state: &AppState) -> Result<ActiveMap, Response<Body>> {
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
    Json(MapResponse {
        id: active.id,
        name: active.name,
        matrix: active.map,
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
    description = "Classic point-to-point BFS route",
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
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let active = active_map(&app_state)?;
    let route = crate::search::bfs(
        &active.map,
        payload.start_point,
        payload.end_point,
        &active.road_points,
        &active.id,
        &mut redis_conn,
    )
    .map_err(|e| bad_request(e.to_string()))?;

    let distance = route.len();
    Ok(Json(GetRouteResponse { route, distance }).into_response())
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
        (status = 200, description = "Current active task, or null if the engineer has none", body = Option<CurrentTaskResponse>),
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
        return Ok(Json(None::<CurrentTaskResponse>).into_response());
    };

    let route: Route = serde_json::from_str(&task.task_route).map_err(|e| {
        tracing::error!(
            error = %e,
            task_id = %task.id,
            "stored task route could not be parsed"
        );
        internal_error()
    })?;

    Ok(Json(Some(CurrentTaskResponse {
        id: task.id.to_string(),
        description: task.description,
        issue: task.issue_type,
        plane_point: Point::new(task.plane_x, task.plane_y),
        plane_uuid: task.plane_id.map(|aircraft_id| aircraft_id.to_string()),
        route,
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

    let aog = payload.aog.or(payload.toggle).ok_or_else(|| {
        unprocessable("aog is required")
    })?;
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

    let (limit, offset) = resolve_page(page)?;
    let active = active_map(&app_state)?;
    let aircraft = load_aog_aircraft(&mut redis_conn, &active)?;
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

    let active = active_map(&app_state)?;
    let (limit, offset) = resolve_page(page)?;
    let aircraft = load_aircraft(&mut redis_conn, &active)?;
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
        .set(is_active.eq(false)) // Указываем поле и новое значение
        .execute(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to update task");
            internal_error()
        })?;

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

    let active = active_map(&app_state)?;
    let plane_uuid =
        Uuid::from_str(&payload.plane_uuid).map_err(|_| bad_request("invalid aircraft UUID"))?;
    let plane_point = load_aircraft_position(&mut redis_conn, &active, plane_uuid)?;
    if !active.map.in_bounds(plane_point) {
        return Err(bad_request("aircraft position is outside of map bounds"));
    }

    // 1. Suitable engineers: specialization comes from the Bayes classifier,
    //    not from the AircraftIssue mapping. Busy engineers are skipped.
    let engineer_type = {
        let model = app_state.classifier.read().map_err(|e| {
            tracing::error!(error = %e, "classifier lock poisoned");
            internal_error()
        })?;
        model
            .classify(&payload.classifier_text)
            .map_err(|e| bad_request(e.to_string()))?
            .engineer_type
    };
    let note = payload
        .note
        .clone()
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            payload
                .description
                .clone()
                .filter(|text| !text.trim().is_empty())
        })
        .ok_or_else(|| unprocessable("note is required"))?;
    let issue = payload.issue;
    let (vehicle, resolution) = if issue == crate::types::enums::AircraftIssue::Other {
        (
            engineer_type.typical_vehicle(),
            engineer_type.default_resolution_time(),
        )
    } else {
        (issue.required_vehicle(), issue.resolution_time())
    };

    use self::tasks::dsl::*;
    let busy_engineers = tasks
        .filter(is_active.eq(true))
        .filter(map_id.is_null().or(map_id.eq(active.id.clone())))
        .select(assigned_engineer);

    let suitable_engineers: Vec<Uuid> = engineers::table
        .filter(engineers::engineer_type.eq(engineer_type))
        .filter(engineers::id.ne_all(busy_engineers))
        .select(engineers::id)
        .load(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load suitable engineers");
            internal_error()
        })?;

    // 2. Their current positions (from Redis).
    let engineers_positions =
        load_engineer_positions(&mut redis_conn, &active, &suitable_engineers)?;
    if engineers_positions.is_empty() {
        return Err(not_found("no suitable engineer available"));
    }

    // 3. Special vehicle, when this specialization always uses one.
    let transport_map = match vehicle {
        Some(vehicle_type) => load_transport_positions(&mut redis_conn, &active, vehicle_type)?,
        None => HashMap::new(),
    };

    // 4. Engineer already standing on the aircraft needs no path.
    //    find_nearest treats that cell as the search start and returns an empty
    //    route, which used to be reported as "engineer was not found".
    let (chosen_uuid, final_route, total_cells) =
        if let Some(engineer_id) = engineers_positions.get(&plane_point).copied() {
            (engineer_id, Route::new(Vec::new()), 0)
        } else {
            let nearest_route = find_nearest(
                &active.map,
                plane_point,
                &active.road_points,
                &engineers_positions,
                &active.id,
                &mut redis_conn,
            )
            .map_err(|e| bad_request(e.to_string()))?;

            let chosen_pos = *nearest_route
                .as_slice()
                .last()
                .ok_or_else(|| not_found("engineer was not found"))?;
            let chosen_uuid = engineers_positions.get(&chosen_pos).copied().ok_or_else(|| {
                tracing::error!(
                    ?chosen_pos,
                    "nearest engineer route ends at unknown position"
                );
                internal_error()
            })?;

            let (final_route, total_cells) = if transport_map.is_empty() {
                let route = crate::search::bfs(
                    &active.map,
                    chosen_pos,
                    plane_point,
                    &active.road_points,
                    &active.id,
                    &mut redis_conn,
                )
                .map_err(|e| bad_request(e.to_string()))?;
                let len = route.len();
                (route, len)
            } else {
                build_route_with_transport(
                    &active,
                    &mut redis_conn,
                    chosen_pos,
                    plane_point,
                    &transport_map,
                    chosen_uuid,
                )?
            };
            (chosen_uuid, final_route, total_cells)
        };

    // 5. Task timing.
    let travel_secs = total_cells as f32 / ENGINEER_SPEED_CELLS_PER_SEC;
    let now = Utc::now();
    let task_ends_at = now + resolution + Duration::from_secs_f32(travel_secs);

    // 6. Persist task.
    let new_task = Task {
        id: Uuid::new_v4(),
        description: note,
        created_at: now,
        ends_at: task_ends_at,
        created_by: auth_user.id,
        assigned_engineer: chosen_uuid,
        issue_type: payload.classifier_text,
        plane_x: plane_point.0,
        plane_y: plane_point.1,
        plane_id: Some(plane_uuid),
        task_route: serde_json::to_string(&final_route).expect("Route is always serializable"),
        is_active: true,
        map_id: Some(active.id.clone()),
    };

    diesel::insert_into(schema::tasks::table)
        .values(&new_task)
        .execute(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to insert new task");
            internal_error()
        })?;

    // 7. Teleport engineer to destination (simulation only).
    redis_conn
        .set(active.engineer_position_key(chosen_uuid), plane_point.as_value())
        .map_err(|e| {
            tracing::error!(error = %e, "failed to teleport engineer");
            internal_error()
        })?;

    let eta_seconds = travel_secs as u64;
    let sla_exceeded = travel_secs >= SLA_LIMIT_SECS;
    Ok(Json(AssignEngineerResponse {
        engineer_uuid: chosen_uuid.to_string(),
        eta_seconds,
        sla_exceeded,
        distance: total_cells as u64,
        time: eta_seconds,
        time_limit_exceeded: sla_exceeded,
        route: final_route,
    })
    .into_response())
}

// ---------------------------------------------------------------------------
// assign_engineer helpers
// ---------------------------------------------------------------------------

/// Loads the current map position of an aircraft (`position:aircraft:{uuid}`).
fn load_aircraft_position(
    redis_conn: &mut redis::Connection,
    map: &ActiveMap,
    id: Uuid,
) -> Result<Point, Response<Body>> {
    match redis_conn.get(map.vehicle_position_key(&VehicleType::Aircraft, id)) {
        Ok(Some(raw)) => Point::from_value(raw).map_err(|e| {
            tracing::error!(%id, error = %e, "aircraft position could not be parsed");
            internal_error()
        }),
        Ok(None) => Err(not_found("aircraft not found")),
        Err(e) => {
            tracing::error!(%id, error = %e, "failed to fetch aircraft position");
            Err(internal_error())
        }
    }
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
        aircraft.push(AircraftEntry { id, point });
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
        aircraft.push(AircraftEntry { id, point });
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

/// Builds the full route "engineer → transport → aircraft".
fn build_route_with_transport(
    map: &ActiveMap,
    redis_conn: &mut redis::Connection,
    engineer_pos: Point,
    plane_pos: Point,
    transport_map: &HashMap<Point, Uuid>,
    engineer_id: Uuid,
) -> Result<(Route, usize), Response<Body>> {
    let to_transport = find_nearest(
        &map.map,
        engineer_pos,
        &map.road_points,
        transport_map,
        &map.id,
        redis_conn,
    )
    .map_err(|e| bad_request(e.to_string()))?;

    if to_transport.is_empty() {
        return Err(not_found("no available transport of required type found"));
    }

    let transport_pos = *to_transport
        .as_slice()
        .last()
        .ok_or_else(|| not_found("transport route is empty"))?;
    let transport_id = transport_map.get(&transport_pos).copied().ok_or_else(|| {
        tracing::error!(?transport_pos, "transport route ends at unknown position");
        internal_error()
    })?;

    let to_plane = crate::search::bfs(
        &map.map,
        transport_pos,
        plane_pos,
        &map.road_points,
        &map.id,
        redis_conn,
    )
    .map_err(|e| bad_request(e.to_string()))?;

    let mut combined = to_transport.as_slice().to_vec();
    let tail = to_plane.as_slice();
    if !tail.is_empty() {
        combined.extend_from_slice(&tail[1..]); // drop duplicated transport start
    }

    tracing::info!(
        engineer = %engineer_id,
        transport = %transport_id,
        "engineer will pick up transport en route to the target"
    );

    let len = combined.len();
    Ok((Route::new(combined), len))
}
