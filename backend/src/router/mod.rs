pub mod auth;
pub mod simulate;

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

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
        dto::{AssignEngineerRequest, AssignEngineerResponse, GetRouteRequest, GetRouteResponse},
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
            tracing::error!(error = %e, "Error during suitable engineers loading");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut keys = HashSet::new();
    let required_vehicle = payload.issue.required_vehicle();

    if let Some(t) = required_vehicle {
        let transport = t.to_string();

        let iter: redis::Iter<'_, String> =
            match redis_conn.scan_match(format!("position:{}:*", transport)) {
                Ok(iter) => iter,
                Err(e) => {
                    tracing::error!("Error during suitable transport loading: {e}");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            };

        for key in iter {
            match key {
                Ok(k) => {
                    keys.insert(k);
                }
                Err(_) => continue,
            }
        }
    }

    // fetch transport points from redis
    let mut transport_map: HashMap<Point, Uuid> = HashMap::new();

    for key in keys {
        match redis_conn.get(&key) {
            Ok(op) => {
                let Some(value) = op else { continue };

                let Some(uuid_str) = key.split(':').last() else {
                    continue;
                };
                let uuid = match Uuid::from_str(uuid_str) {
                    Ok(u) => u,
                    Err(e) => {
                        tracing::warn!(error = %e, key = %key, "malformed transport uuid in redis key, skipped");
                        continue;
                    }
                };

                match Point::from_value(value) {
                    Ok(p) => {
                        transport_map.insert(p, uuid);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, key = %key, "malformed transport position, skipped");
                    }
                }
            }
            Err(e) => {
                tracing::error!("Error during loading transport point: {e}");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };
    }

    // fetch suitable engineers' positions
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
            Err(e) => {
                tracing::error!(error = %e, "error happened during fetching engineer positions");
            },
        };
    });

    if engineers_positions.is_empty() {
        return (StatusCode::NOT_FOUND, "no suitable engineer available").into_response();
    }

    let nearest_engineer_probe = match find_nearest(
        &app_state.map,
        payload.plane_point,
        &app_state.road_points,
        &engineers_positions,
        &mut redis_conn,
    ) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let chosen_pos = match nearest_engineer_probe.get_vec().last() {
        Some(p) => *p,
        None => return (StatusCode::NOT_FOUND, "engineer was not found").into_response(),
    };
    let chosed_uuid = *engineers_positions.get(&chosen_pos).unwrap();

    let (final_route, total_cells): (Route, usize) = if !transport_map.is_empty() {
        let route_to_transport = match find_nearest(
            &app_state.map,
            chosen_pos,
            &app_state.road_points,
            &transport_map,
            &mut redis_conn,
        ) {
            Ok(r) if !r.get_vec().is_empty() => r,
            Ok(_) => {
                return (
                    StatusCode::NOT_FOUND,
                    "no available transport of required type found",
                )
                    .into_response();
            }
            Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        };

        let transport_point = match route_to_transport.get_vec().last() {
            Some(p) => *p,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    "no available transport of required type found",
                )
                    .into_response();
            }
        };

        let transport_uuid = match transport_map.get(&transport_point) {
            Some(u) => *u,
            None => return (StatusCode::BAD_REQUEST, "transport was not found").into_response(),
        };

        let route_transport_to_target = match crate::search::bfs(
            &app_state.map,
            transport_point,
            payload.plane_point,
            &app_state.road_points,
            &mut redis_conn,
        ) {
            Ok(r) => r,
            Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        };

        let mut combined: Vec<Point> = route_to_transport.get_vec().clone();
        let mut tail = route_transport_to_target.get_vec().clone();
        if !tail.is_empty() {
            tail.remove(0);
        }
        combined.extend(tail);

        tracing::info!(
            engineer = %chosed_uuid,
            transport = %transport_uuid,
            "engineer will pick up required transport en route to the target"
        );

        let len = combined.len();
        (Route::new(combined), len)
    } else {
        let direct_route = match crate::search::bfs(
            &app_state.map,
            chosen_pos,
            payload.plane_point,
            &app_state.road_points,
            &mut redis_conn,
        ) {
            Ok(r) => r,
            Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        };
        let len = direct_route.get_vec().len();
        (direct_route, len)
    };

    const SPEED: f32 = 0.05;
    let required_time = total_cells as f32 / SPEED;

    let utc_now = Utc::now();

    let new_task = Task {
        id: Uuid::new_v4(),
        description: payload.description,
        created_at: utc_now,
        ends_at: utc_now + payload.issue.resolution_time() + Duration::from_secs_f32(required_time),
        created_by: auth_user.id,
        assigned_engineer: chosed_uuid,
        issue_type: payload.issue,
        is_active: true,
    };

    if let Err(e) = diesel::insert_into(schema::tasks::table)
        .values(&new_task)
        .execute(&mut pg_conn)
    {
        tracing::error!(error = %e, "error happened during creating new task, should not be an unique key violation because of random UUID generation here");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // teleport engineer to a point (SIMULATION PURPOSES ONLY)
    match redis_conn.set(
        format!("position:{}", chosed_uuid),
        payload.plane_point.as_value(),
    ) {
        Ok(_) => (),
        Err(e) => {
            tracing::error!(error = %e, "error happened during teleporting engineer to a target point");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    Json(AssignEngineerResponse {
        engineer_uuid: chosed_uuid.to_string(),
        time: required_time as u64,
        time_limit_exceeded: required_time >= 900 as f32,
        route: final_route,
    })
    .into_response()
}
