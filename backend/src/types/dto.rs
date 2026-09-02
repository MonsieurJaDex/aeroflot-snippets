use num_traits::PrimInt;
use serde::Serialize;

use crate::{error, types::map::MapMatrix};

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
