use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::types::{
    enums::AircraftIssue,
    map::{MapMatrix, Point, Route},
};

#[derive(Debug, Serialize)]
pub struct MatrixResponse {
    pub width: usize,
    pub height: usize,
    pub matrix: MapMatrix,
}

impl MatrixResponse {
    pub fn new(matrix: MapMatrix) -> anyhow::Result<Self> {
        if matrix.0.is_empty() {
            return Err(anyhow!("got empty matrix".to_string()));
        }

        let width = matrix.0[0].len();
        let height = matrix.0.len();
        Ok(Self {
            matrix,
            width,
            height,
        })
    }
}

#[derive(Serialize, ToSchema)]
pub struct GetRouteResponse {
    pub route: Route,
    pub distance: usize,
}

#[derive(Deserialize, ToSchema)]
pub struct GetRouteRequest {
    pub start_point: Point,
    pub end_point: Point,
}

#[derive(Deserialize, ToSchema)]
pub struct AssignEngineerRequest {
    pub dispatcher_uuid: String,
    pub issue: AircraftIssue,
    pub plane_point: Point,
    pub description: String,
}

#[derive(Serialize, ToSchema)]
pub struct AssignEngineerResponse {
    pub engineer_uuid: String,
    pub time: f32,
    pub time_limit_exceeded: bool,
    pub route: Route,
}
