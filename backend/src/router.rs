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

// #[utoipa::path(
//     post,
//     path="/api/assign",
//     description="Assign an engineer to requested point",
//     responses(
//         (status=200,
//         description="Requested engineer found and assigned to target point",
//         body=GetRouteResponse<i8>
//         )
//     )
// )]
// pub async fn assign_engineer<T: PrimInt + Serialize>(
//     State(map): State<Arc<MapMatrix<T>>>,
// ) -> Response {
//     todo!()
// }

#[utoipa::path(
    post,
    path="/api/getRoute",
    description="Classic Point-to-Point BFS",
    request_body=GetRouteRequest,
    responses(
        (
            status=200,
            description="Successful path findingm returning a point sequence as route",
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
) -> Result<(StatusCode, Json<GetRouteResponse>), (StatusCode, String)> {
    let res = crate::search::bfs(
        &app_state.map,
        Point::new(payload.start_point.0, payload.start_point.1),
        Point::new(payload.end_point.0, payload.end_point.1),
        &app_state.road_points,
    );

    match res {
        Ok(r) => {
            let len = r.len();
            Ok((
                StatusCode::OK,
                Json(GetRouteResponse {
                    route: r,
                    distance: len,
                }),
            ))
        }
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}
