use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Response, StatusCode},
    response::IntoResponse,
};
use diesel::{QueryDsl, RunQueryDsl};
use redis::TypedCommands;
use uuid::Uuid;

use crate::{
    database::schema,
    types::{config::AppState, dto::UpdateEngineerPositionRequest, map::Point},
};

// add possibility to move engineer and control simulation params from API (speed etc.)
#[utoipa::path(
    post,
    path="/api/simulate/update_engineer_position",
    description="Manual engineer position update",
    request_body=UpdateEngineerPositionRequest,
    responses(
        (status=200, description="Successful update", body=String),
        (status=400, description="Invalid request format", body=String),
        (status=500, description="Server-side error", body=String)
    )
)]
pub async fn update_engineer_position_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<UpdateEngineerPositionRequest>,
) -> Response<Body> {
    let uuid: Uuid = match Uuid::parse_str(&payload.id) {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "invalid ID format, parsing faile").into_response();
        }
    };

    if payload.new_point.1 < 0 || payload.new_point.1 as usize > app_state.map.0.len() {
        return (StatusCode::BAD_REQUEST, "invalid Y point coordinate").into_response();
    }

    if let Some(row) = app_state.map.0.get(0) {
        if payload.new_point.0 < 0 || payload.new_point.0 as usize > row.len() {
            return (StatusCode::BAD_REQUEST, "invalid X point coordinate").into_response();
        }
    }

    let mut redis_conn = match app_state.redis_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    match redis_conn.set(format!("position:{}", uuid), payload.new_point.as_value()) {
        Ok(_) => (),
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    StatusCode::OK.into_response()
}

#[utoipa::path(
    post,
    path="/api/simulate/get_engineers_positions",
    description="Get all engineers with their positions",
    responses(
        (status=200, description="Successful fetch", body=String),
        (status=500, description="Server-side error", body=String)
    )
)]
pub async fn get_engineers_positions(State(app_state): State<Arc<AppState>>) -> Response<Body> {
    let mut pg_conn = match app_state.db_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting postgres connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut redis_conn = match app_state.redis_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let ids: Vec<Uuid> = match schema::engineers::table
        .select(schema::engineers::id)
        .load(&mut pg_conn)
    {
        Err(e) => {
            tracing::error!(error = %e, "error happened during gathering all engineers");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        Ok(u) => u,
    };

    let mut pos = Vec::new();
    for id in &ids {
        match redis_conn.get(format!("position:{}", id)) {
            Ok(r) => match r {
                Some(res) => pos.push(Some(Point::from_value(res).unwrap())),
                None => pos.push(None),
            },
            Err(e) => {
                tracing::error!(error = %e, "error happened during position extracting");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };
    }

    let response: Vec<(&Uuid, Option<Point>)> = ids.iter().zip(pos).map(|(i, p)| (i, p)).collect();

    Json(response).into_response()
}
