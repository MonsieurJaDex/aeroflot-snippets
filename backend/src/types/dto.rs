use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    error,
    types::map::{MapMatrix, Point, Route},
};

#[derive(Debug, Serialize)]
pub struct MatrixResponse {
    pub width: usize,
    pub height: usize,
    pub matrix: MapMatrix,
}

impl MatrixResponse {
    pub fn new(matrix: MapMatrix) -> Result<Self, Box<dyn std::error::Error>> {
        if matrix.0.is_empty() {
            return Err(Box::new(error::CommonErrors::InvalidArgument(
                "got empty matrix".to_string(),
            )));
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
    // TODO: add assigned engineer object here
    pub route: Route,
    pub distance: usize,
}

#[derive(Deserialize, ToSchema)]
pub struct GetRouteRequest {
    pub start_point: Point,
    pub end_point: Point,
}
