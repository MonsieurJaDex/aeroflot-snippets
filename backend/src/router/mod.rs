//! HTTP router: common helpers, state and public handler definitions.

pub mod auth;
pub mod geo;

use std::{collections::HashMap, str::FromStr, sync::Arc, time::Duration};

use axum::{
    Extension, Json,
    body::Body,
    extract::State,
    http::{Response, StatusCode},
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
    search::find_nearest,
    types::{
        config::AppState,
        dto::{
            AircraftEntry, AircraftListResponse, AogListResponse, AssignEngineerRequest,
            AssignEngineerResponse, CurrentTaskAogRequest, CurrentTaskAogResponse,
            CurrentTaskResponse, GetRouteRequest,
        },
        enums::{UserRole, VehicleType},
        map::{MapMatrix, Point, Route},
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

/// Returns a 500 response with an empty body.
pub(crate) fn internal_error() -> Response<Body> {
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// Returns a 400 response with the given message.
pub(crate) fn bad_request(msg: impl Into<String>) -> Response<Body> {
    (StatusCode::BAD_REQUEST, msg.into()).into_response()
}

/// Returns a 401 response with the given message.
pub(crate) fn unauthorized(msg: impl Into<String>) -> Response<Body> {
    (StatusCode::UNAUTHORIZED, msg.into()).into_response()
}

/// Returns a 403 response with the given message.
pub(crate) fn forbidden(msg: impl Into<String>) -> Response<Body> {
    (StatusCode::FORBIDDEN, msg.into()).into_response()
}

/// Returns a 404 response with the given message.
pub(crate) fn not_found(msg: impl Into<String>) -> Response<Body> {
    (StatusCode::NOT_FOUND, msg.into()).into_response()
}

// ---------------------------------------------------------------------------
// Map
// ---------------------------------------------------------------------------

/// Returns the current airport map matrix.
#[utoipa::path(
    get,
    path = "/api/map",
    responses(
        (status = 200, description = "Current map matrix", body = MapMatrix),
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer access token"),
    ),
)]
pub async fn get_map(State(app_state): State<Arc<AppState>>) -> Response<Body> {
    Json(&app_state.map).into_response()
}

// ---------------------------------------------------------------------------
// Route calculation
// ---------------------------------------------------------------------------

/// Computes the shortest path between two points on the map.
#[utoipa::path(
    post,
    path = "/api/getRoute",
    description = "Classic point-to-point BFS route",
    request_body = GetRouteRequest,
    responses(
        (status = 200, description = "Successfully found a route", body = Route),
        (status = 400, description = "Invalid input data", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer access token"),
    ),
)]
pub async fn get_route(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<GetRouteRequest>,
) -> Result<Response<Body>, Response<Body>> {
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let route = crate::search::bfs(
        &app_state.map,
        payload.start_point,
        payload.end_point,
        &app_state.road_points,
        &mut redis_conn,
    )
    .map_err(|e| bad_request(e.to_string()))?;

    Ok((StatusCode::OK, Json(route)).into_response())
}

// ---------------------------------------------------------------------------
// Current task
// ---------------------------------------------------------------------------

/// Fetches the currently active task for the authenticated engineer.
#[utoipa::path(
    get,
    path = "/api/tasks/current",
    responses(
        (status = 200, description = "Current active task", body = Option<CurrentTaskResponse>),
        (status = 403, description = "Authenticated user is not an engineer", body = String),
        (status = 404, description = "Engineer has no active task", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer access token"),
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

    use self::tasks::dsl::*;
    let task = tasks
        .filter(assigned_engineer.eq(auth_user.id))
        .filter(is_active.eq(true))
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
    post,
    path = "/api/tasks/current/aog",
    request_body = CurrentTaskAogRequest,
    responses(
        (status = 200, description = "AOG status removed", body = CurrentTaskAogResponse),
        (status = 201, description = "AOG status set", body = CurrentTaskAogResponse),
        (status = 400, description = "Current task has no aircraft", body = String),
        (status = 403, description = "Authenticated user is not an engineer", body = String),
        (status = 404, description = "Engineer has no active task", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer access token"),
    ),
)]
pub async fn current_task_aog(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Json(payload): Json<CurrentTaskAogRequest>,
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

    use self::tasks::dsl::*;
    let task = tasks
        .filter(assigned_engineer.eq(auth_user.id))
        .filter(is_active.eq(true))
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

    let key = format!("aog:{plane_uuid}");
    if payload.toggle {
        redis_conn.set(&key, "1").map_err(|e| {
            tracing::error!(%plane_uuid, error = %e, "failed to set aircraft AOG status");
            internal_error()
        })?;

        return Ok((
            StatusCode::CREATED,
            Json(CurrentTaskAogResponse {
                plane_uuid: plane_uuid.to_string(),
                aog: true,
            }),
        )
            .into_response());
    }

    redis_conn.del(&key).map_err(|e| {
        tracing::error!(%plane_uuid, error = %e, "failed to clear aircraft AOG status");
        internal_error()
    })?;

    Ok((
        StatusCode::OK,
        Json(CurrentTaskAogResponse {
            plane_uuid: plane_uuid.to_string(),
            aog: false,
        }),
    )
        .into_response())
}

/// Lists aircraft UUIDs that currently have an AOG status.
#[utoipa::path(
    get,
    path = "/api/tasks/aog",
    responses(
        (status = 200, description = "Aircraft currently marked AOG", body = AogListResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer access token"),
    ),
)]
pub async fn list_aog(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can view AOG aircraft"));
    }

    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let plane_uuids = load_aog_aircraft(&mut redis_conn)?;
    Ok(Json(AogListResponse { plane_uuids }).into_response())
}

/// Lists every aircraft currently stored in Redis.
#[utoipa::path(
    get,
    path = "/api/aircraft",
    responses(
        (status = 200, description = "Current aircraft", body = AircraftListResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer access token"),
    ),
)]
pub async fn list_aircraft(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<Body>, Response<Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can view aircraft"));
    }

    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let aircraft = load_aircraft(&mut redis_conn)?;
    Ok(Json(AircraftListResponse { aircraft }).into_response())
}

/// Fetches the currently active task for the authenticated engineer.
#[utoipa::path(
    post,
    path = "/api/tasks/current/complete",
    description = "Marks the current active task of the authenticated engineer as completed",
    responses(
        (status = 200, description = "Task marked as completed"),
        (status = 403, description = "Authenticated user is not an engineer", body = String),
        (status = 404, description = "Engineer has no active task", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer access token"),
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

    use self::tasks::dsl::*;
    let task = tasks
        .filter(assigned_engineer.eq(auth_user.id))
        .filter(is_active.eq(true))
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
// Engineer assignment
// ---------------------------------------------------------------------------

/// Creates a new task and assigns the most suitable engineer.
#[utoipa::path(
    post,
    path = "/api/assign",
    description = "Find the nearest suitable engineer, optionally pick up a special vehicle, and assign to an aircraft",
    request_body = AssignEngineerRequest,
    responses(
        (status = 200, description = "Task created, engineer assigned", body = AssignEngineerResponse),
        (status = 400, description = "Invalid input or route error", body = String),
        (status = 403, description = "Authenticated user is not a dispatcher", body = String),
        (status = 404, description = "No suitable engineer / transport found", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer access token"),
    ),
)]
pub async fn assign_engineer(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Json(payload): Json<AssignEngineerRequest>,
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

    let plane_uuid =
        Uuid::from_str(&payload.plane_uuid).map_err(|_| bad_request("invalid aircraft UUID"))?;
    let plane_point = load_aircraft_position(&mut redis_conn, plane_uuid)?;
    if !app_state.map.in_bounds(plane_point) {
        return Err(bad_request("aircraft position is outside of map bounds"));
    }

    // 1. Suitable engineers (of the right specialization, not already busy with
    //    the same issue type).
    use self::tasks::dsl::*;
    let busy_engineers = tasks.filter(is_active.eq(true)).select(assigned_engineer);

    let suitable_engineers: Vec<Uuid> = engineers::table
        .filter(engineers::engineer_type.eq(payload.issue.responsible_engineer()))
        .filter(engineers::id.ne_all(busy_engineers))
        .select(engineers::id)
        .load(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load suitable engineers");
            internal_error()
        })?;

    // 2. Their current positions (from Redis).
    let engineers_positions = load_engineer_positions(&mut redis_conn, &suitable_engineers)?;
    if engineers_positions.is_empty() {
        return Err(not_found("no suitable engineer available"));
    }

    // 3. Special vehicles, if the issue requires one.
    let transport_map = match payload.issue.required_vehicle() {
        Some(vehicle_type) => load_transport_positions(&mut redis_conn, vehicle_type)?,
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
                &app_state.map,
                plane_point,
                &app_state.road_points,
                &engineers_positions,
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
                    &app_state.map,
                    chosen_pos,
                    plane_point,
                    &app_state.road_points,
                    &mut redis_conn,
                )
                .map_err(|e| bad_request(e.to_string()))?;
                let len = route.len();
                (route, len)
            } else {
                build_route_with_transport(
                    &app_state,
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
    let task_ends_at = now + payload.issue.resolution_time() + Duration::from_secs_f32(travel_secs);

    // 6. Persist task.
    let new_task = Task {
        id: Uuid::new_v4(),
        description: payload.description,
        created_at: now,
        ends_at: task_ends_at,
        created_by: auth_user.id,
        assigned_engineer: chosen_uuid,
        issue_type: payload.issue,
        plane_x: plane_point.0,
        plane_y: plane_point.1,
        plane_id: Some(plane_uuid),
        task_route: serde_json::to_string(&final_route).expect("Route is always serializable"),
        is_active: true,
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
        .set(format!("position:{}", chosen_uuid), plane_point.as_value())
        .map_err(|e| {
            tracing::error!(error = %e, "failed to teleport engineer");
            internal_error()
        })?;

    Ok(Json(AssignEngineerResponse {
        engineer_uuid: chosen_uuid.to_string(),
        time: travel_secs as u64,
        time_limit_exceeded: travel_secs >= SLA_LIMIT_SECS,
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
    id: Uuid,
) -> Result<Point, Response<Body>> {
    match redis_conn.get(format!("position:aircraft:{id}")) {
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
fn load_aircraft(redis_conn: &mut redis::Connection) -> Result<Vec<AircraftEntry>, Response<Body>> {
    let keys: Vec<String> = redis_conn
        .scan_match("position:aircraft:*")
        .map_err(|e| {
            tracing::error!(error = %e, "failed to scan aircraft keys");
            internal_error()
        })?
        .filter_map(Result::ok)
        .collect();

    let mut aircraft = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(uuid_str) = key.strip_prefix("position:aircraft:") else {
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

/// Loads aircraft UUIDs that currently have an AOG flag (`aog:{uuid}`).
fn load_aog_aircraft(redis_conn: &mut redis::Connection) -> Result<Vec<String>, Response<Body>> {
    let keys: Vec<String> = redis_conn
        .scan_match("aog:*")
        .map_err(|e| {
            tracing::error!(error = %e, "failed to scan AOG keys");
            internal_error()
        })?
        .filter_map(Result::ok)
        .collect();

    let mut plane_uuids = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(uuid_str) = key.strip_prefix("aog:") else {
            continue;
        };
        match Uuid::from_str(uuid_str) {
            Ok(uuid) => plane_uuids.push(uuid.to_string()),
            Err(e) => {
                tracing::warn!(%key, error = %e, "malformed AOG key, skipped");
            }
        }
    }

    plane_uuids.sort();
    Ok(plane_uuids)
}

/// Loads current engineer positions from Redis into a `Point -> Uuid` map.
fn load_engineer_positions(
    redis_conn: &mut redis::Connection,
    ids: &[Uuid],
) -> Result<HashMap<Point, Uuid>, Response<Body>> {
    let mut positions = HashMap::with_capacity(ids.len());

    for &id in ids {
        match redis_conn.get(format!("position:{}", id)) {
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
    vehicle_type: VehicleType,
) -> Result<HashMap<Point, Uuid>, Response<Body>> {
    let mut positions = HashMap::new();

    let keys: Vec<String> = redis_conn
        .scan_match(format!("position:{}:*", vehicle_type))
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
    state: &AppState,
    redis_conn: &mut redis::Connection,
    engineer_pos: Point,
    plane_pos: Point,
    transport_map: &HashMap<Point, Uuid>,
    engineer_id: Uuid,
) -> Result<(Route, usize), Response<Body>> {
    let to_transport = find_nearest(
        &state.map,
        engineer_pos,
        &state.road_points,
        transport_map,
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
        &state.map,
        transport_pos,
        plane_pos,
        &state.road_points,
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
