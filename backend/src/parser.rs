use std::{error::Error, fmt::Debug, fs};

use num_traits::PrimInt;
use serde::{Deserialize, de::DeserializeOwned};

use crate::{
    error,
    types::{self, MapMatrix},
};

// map parser (tmj -> MapMatrix)
pub fn parse_map<T: DeserializeOwned + PrimInt + Debug + Copy>(
    path: &str,
) -> Result<types::MapMatrix<T>, Box<dyn Error>> {
    let path = path.trim();
    let content: String = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return Err(Box::new(e)),
    };

    let json_data: serde_json::Value = serde_json::from_str(&content)?;

    let layers: Vec<types::TiledLayer<T, u64>> =
        serde_json::from_value(json_data["layers"].clone())?;

    if layers.len() == 0 {
        return Err(Box::new(error::CommonErrors::IncorrectLayer(
            "empty layers".to_string(),
        )));
    }

    let mut matrix: MapMatrix<T> = MapMatrix::new(
        layers[0].width as usize,
        layers[0].height as usize,
        T::zero(),
    );

    for layer in layers {
        let chunks = layer.data.chunks(layer.width as usize);

        for (matrix_row, chunk_row) in matrix.0.iter_mut().zip(chunks) {
            for (matrix_cell, chunk_cell) in matrix_row.iter_mut().zip(chunk_row) {
                if chunk_cell.eq(&T::zero()) {
                    continue;
                }
                matrix_cell.clone_from(chunk_cell);
            }
        }
    }

    Ok(matrix)
}
