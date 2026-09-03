use num_traits::PrimInt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    error,
    types::map::{MapMatrix, Point, Route},
};

#[derive(Debug, Serialize)]
pub struct MatrixResponse<T: PrimInt + Copy> {
    pub width: usize,
    pub height: usize,
    pub matrix: MapMatrix<T>,
}

impl<T: PrimInt + Copy> MatrixResponse<T> {
    pub fn new(matrix: MapMatrix<T>) -> Result<Self, Box<dyn std::error::Error>> {
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
pub struct GetRouteResponse<T: PrimInt + Copy + ToSchema> {
    // TODO: add assigned engineer object here
    pub route: Route<T>,
    pub distance: usize,
}

#[derive(Deserialize, ToSchema)]
pub struct GetRouteRequest<T: PrimInt> {
    #[schema(example = json!([0, 0]))]
    pub start_point: [T; 2],

    #[schema(example = json!([1, 1]))]
    pub end_point: [T; 2],
}
