pub mod auth;
pub mod simulate;

use std::{collections::HashMap, sync::Arc, time::Duration};

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
        dto::{AssignEngineerRequest, AssignEngineerResponse, CurrentTaskResponse, GetRouteRequest, GetRouteResponse},
        enums::UserRole,
        map::{MapMatrix, Point, Route},
    },
};

#[utoipa::path(
    get,
    path="/api/map",
    responses(
        (status=200, description="Return actual map", body=MapMatrix)
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer authorization access token")
    )
)]
pub async fn get_map(State(app_state): State<Arc<AppState>>) -> Response<Body> {
    Json(&app_state.map).into_response()
}

#[utoipa::path(
    post,
    path="/api/getRoute",
    description="Classic Point-to-Point BFS",
    request_body=GetRouteRequest,
    responses(
        (
            status=200,
            description="Successful path finding, returning a point sequence as route",
            body=GetRouteResponse
        ),
        (
            status=400,
            description="Error caused by incorrect input data",
            body=String
        )
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer authorization access token")
    )
)]
pub async fn get_route(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<GetRouteRequest>,
) -> Response<Body> {
    let mut redis_conn = match app_state.redis_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let res = crate::search::bfs(
        &app_state.map,
        Point::new(payload.start_point.0, payload.start_point.1),
        Point::new(payload.end_point.0, payload.end_point.1),
        &app_state.road_points,
        &mut redis_conn,
    );

    match res {
        Ok(r) => (StatusCode::OK, Json(r)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[utoipa::path(
    get,
    path="/api/tasks/current",
    responses((status=200, description="Current active task for the authenticated engineer", body=Option<CurrentTaskResponse>)),
    params(("Authorization" = String, Header, description="Bearer authorization access token"))
)]
pub async fn get_current_task(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Response<Body> {
    if auth_user.role != UserRole::Engineer {
        return (StatusCode::FORBIDDEN, "only engineers can view their tasks").into_response();
    }

    let mut pg_conn = match app_state.db_pool.get() {
        Ok(connection) => connection,
        Err(error) => {
            tracing::error!(error = %error, "error during postgres connection extraction");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    use self::tasks::dsl::*;
    let task = match tasks
        .filter(assigned_engineer.eq(auth_user.id))
        .filter(is_active.eq(true))
        .order(created_at.desc())
        .select(Task::as_select())
        .first::<Task>(&mut pg_conn)
        .optional()
    {
        Ok(task) => task,
        Err(error) => {
            tracing::error!(error = %error, "error during current task loading");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    match task {
        Some(task) => match serde_json::from_str::<Route>(&task.task_route) {
            Ok(route) => Json(Some(CurrentTaskResponse {
                id: task.id.to_string(),
                description: task.description,
                issue: task.issue_type,
                plane_point: Point::new(task.plane_x, task.plane_y),
                route,
                is_accepted: task.is_accepted,
            }))
            .into_response(),
            Err(error) => {
                tracing::error!(error = %error, task_id = %task.id, "stored task route could not be parsed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        },
        None => Json(Option::<CurrentTaskResponse>::None).into_response(),
    }
}

#[utoipa::path(
    post,
    path="/api/tasks/current/accept",
    responses((status=200, description="Current task accepted"), (status=404, description="No active task found")),
    params(("Authorization" = String, Header, description="Bearer authorization access token"))
)]
pub async fn accept_current_task(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Response<Body> {
    if auth_user.role != UserRole::Engineer {
        return (StatusCode::FORBIDDEN, "only engineers can accept tasks").into_response();
    }

    let mut pg_conn = match app_state.db_pool.get() {
        Ok(connection) => connection,
        Err(error) => {
            tracing::error!(error = %error, "error during postgres connection extraction");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    use self::tasks::dsl::*;
    match diesel::update(tasks.filter(assigned_engineer.eq(auth_user.id)).filter(is_active.eq(true)))
        .set(is_accepted.eq(true))
        .execute(&mut pg_conn)
    {
        Ok(0) => (StatusCode::NOT_FOUND, "no active task found").into_response(),
        Ok(_) => StatusCode::OK.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "error during current task acceptance");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[utoipa::path(
    post,
    path="/api/tasks/current/close",
    responses((status=200, description="Current task closed"), (status=403, description="Only dispatchers can close tasks"), (status=404, description="No active task found")),
    params(("Authorization" = String, Header, description="Bearer authorization access token"))
)]
pub async fn close_current_task(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Response<Body> {
    if auth_user.role != UserRole::Dispatcher {
        return (StatusCode::FORBIDDEN, "only dispatchers can close tasks").into_response();
    }

    let mut pg_conn = match app_state.db_pool.get() {
        Ok(connection) => connection,
        Err(error) => {
            tracing::error!(error = %error, "error during postgres connection extraction");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    use self::tasks::dsl::*;
    match diesel::update(tasks.filter(created_by.eq(auth_user.id)).filter(is_active.eq(true)))
        .set(is_active.eq(false))
        .execute(&mut pg_conn)
    {
        Ok(0) => (StatusCode::NOT_FOUND, "no active task found").into_response(),
        Ok(_) => StatusCode::OK.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "error during task closing");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[utoipa::path(
    post,
    path="/api/assign",
    description="Find nearest suitable engineer and assign to an aircraft",
    request_body=AssignEngineerRequest,
    responses(
        (
            status=200,
            description="Successful path finding, returning a point sequence as route",
            body=AssignEngineerResponse,
        ),
        (
            status=400,
            description="Some error happened during task processing",
            body=String
        ),
        (
            status=404,
            description="Suitable engineer was not found",
            body=String
        ),
        (
            status=422,
            description="Provided JSON request has an error and cannot be parsed",
            body=String
        ),
        (
            status=500,
            description="Server-side error",
            body=String
        )
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer authorization access token")
    )
)]
#[axum::debug_handler]
pub async fn assign_engineer(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Json(payload): Json<AssignEngineerRequest>,
) -> Response<Body> {
    if auth_user.role != UserRole::Dispatcher {
        return (
            StatusCode::FORBIDDEN,
            "only dispatchers can assign engineers",
        )
            .into_response();
    }

    let mut pg_conn = match app_state.db_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(
                error = %e,
                "Error during extracting postgres connection from pool",
            );
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut redis_conn = match app_state.redis_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(
                error = %e,
                "Error during extracting redis connection from pool"
            );
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    use self::tasks::dsl::*;

    let active_engineers = tasks
        .filter(is_active.eq(true))
        .filter(issue_type.eq(payload.issue))
        .select(assigned_engineer);

    let suitable_engineers: Vec<Uuid> = match engineers::table
        .filter(engineers::engineer_type.eq(payload.issue.responsible_engineer()))
        .filter(engineers::id.ne_all(active_engineers))
        .select(engineers::id)
        .load(&mut pg_conn)
    {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Error during suitable engineers loading");
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let mut engineers_positions: HashMap<Point, Uuid> = HashMap::new();
    suitable_engineers.iter().for_each(|u| {
        match redis_conn.get(format!("position:{}", u)) {
            Ok(res) => match res {
                Some(p) => {
                    let p = Point::from_value(p);
                    if p.is_ok() {
                        engineers_positions.insert(p.unwrap(), *u);
                    } else {
                        tracing::warn!(uuid = %u, "engineer's position was found, but was not parsed well")
                    }
                },
                None =>
                    tracing::warn!(uuid = %u, "engineer's position was not found in redis. Skipped."),
            },
            Err(error) => {
                tracing::warn!(error = %error, uuid = %u, "engineer's position could not be loaded. Skipped.");
            }
        };
    });

    let found_route = match find_nearest(
        &app_state.map,
        payload.plane_point,
        &app_state.road_points,
        &engineers_positions,
        &mut redis_conn,
    ) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let chosen_pos = match found_route.get_vec().first() {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "engineer was not found").into_response(),
    };

    let chosed_uuid = engineers_positions.get(chosen_pos).unwrap();
    const SPEED: f32 = 0.05;

    let required_time = found_route.get_vec().len().saturating_sub(1) as f32 / SPEED;

    let utc_now = Utc::now();

    let new_task = Task {
        id: Uuid::new_v4(),
        description: payload.description,
        created_at: utc_now,
        ends_at: utc_now + payload.issue.resolution_time() + Duration::from_secs_f32(required_time),
        created_by: auth_user.id,
        assigned_engineer: *chosed_uuid,
        issue_type: payload.issue,
        plane_x: payload.plane_point.0,
        plane_y: payload.plane_point.1,
        task_route: serde_json::to_string(&found_route).expect("route is always serializable"),
        is_accepted: false,
        is_active: true,
    };

    if let Err(e) = diesel::insert_into(schema::tasks::table)
        .values(&new_task)
        .execute(&mut pg_conn)
    {
        tracing::error!(error = %e, "error happened during creating new task, should not be an unique key violation because of random UUID generation here");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(AssignEngineerResponse {
        engineer_uuid: chosed_uuid.to_string(),
        time: required_time as u64,
        time_limit_exceeded: required_time >= 900 as f32,
        route: found_route,
    })
    .into_response()
}
