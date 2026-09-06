use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Response, StatusCode},
    response::IntoResponse,
};

use crate::types::{
    config::AppState,
    dto::{GetRouteRequest, GetRouteResponse},
    map::{MapMatrix, Point},
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
