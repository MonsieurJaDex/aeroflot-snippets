use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Response, StatusCode},
    response::IntoResponse,
};

use diesel::prelude::*;

use redis::TypedCommands;
use uuid::Uuid;

use crate::{
    database::schema::{engineers, tasks},
    search::find_nearest,
    types::{
        config::AppState,
        dto::{AssignEngineerRequest, AssignEngineerResponse, GetRouteRequest, GetRouteResponse},
        map::{MapMatrix, Point},
    },
};

#[utoipa::path(
    get,
    path="/api/map",
    responses(
        (status=200, description="Return actual map", body=MapMatrix)
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
    )
)]
pub async fn get_route(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<GetRouteRequest>,
) -> Response<Body> {
    let res = crate::search::bfs(
        &app_state.map,
        Point::new(payload.start_point.0, payload.start_point.1),
        Point::new(payload.end_point.0, payload.end_point.1),
        &app_state.road_points,
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
    )
)]
pub async fn assign_engineer(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<AssignEngineerRequest>,
) -> Response<Body> {
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
            Err(_) => todo!(),
        };
    });

    let route = match find_nearest(
        &app_state.map,
        payload.plane_point,
        &app_state.road_points,
        &engineers_positions,
    ) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let chosen_pos = match route.get_vec().first() {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "engineer was not found").into_response(),
    };

    let chosed_uuid = engineers_positions.get(chosen_pos).unwrap();
    const SPEED: f32 = 0.05;

    let required_time = route.len() as f32 / SPEED;

    // TODO: after auth, automaticly evaluate dispatcher uuid

    Json(AssignEngineerResponse {
        engineer_uuid: chosed_uuid.to_string(),
        time: required_time,
        time_limit_exceeded: required_time >= 900 as f32,
        route: route,
    })
    .into_response()
}
