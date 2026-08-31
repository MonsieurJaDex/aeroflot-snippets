use std::{
    error::Error,
    fmt::Debug,
    fs::File,
    hash::Hash,
    io::{BufReader, Write},
};

use num_traits::PrimInt;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    error,
    types::{self, JsonMatrix, MapMatrix, TmjDto},
};

// map parser (tmj -> MapMatrix)
pub fn parse_map<T>(
    path: &str,
    save_path: Option<&str>,
) -> Result<types::MapMatrix<T>, Box<dyn Error>>
where
    T: DeserializeOwned + Serialize + PrimInt + Debug + Copy + Hash,
{
    let path = path.trim();
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let raw_map: TmjDto<T, u64> = serde_json::from_reader(reader)?;

    let layers: Vec<types::TiledLayer<T, u64>> = raw_map.layers;

    if layers.is_empty() {
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
        if layer.width == 0 {
            return Err(Box::new(error::CommonErrors::IncorrectFileContent(
                format!("Zero layer width catched at layer: {}", layer.name),
            )));
        }

        let chunks = layer.data.chunks(layer.width as usize);

        for (matrix_row, chunk_row) in matrix.0.iter_mut().zip(chunks) {
            for (matrix_cell, chunk_cell) in matrix_row.iter_mut().zip(chunk_row) {
                if chunk_cell.is_zero() {
                    continue;
                }
                *matrix_cell = *chunk_cell
            }
        }
    }

    if let Some(p) = save_path {
        let mut file = File::create(p)?;

        let stringified = serde_json::to_string(&matrix)?;
        let json_matrix = JsonMatrix::new(stringified);

        let wrapper_str = serde_json::to_string(&json_matrix)?;
        file.write_all(wrapper_str.as_bytes())?;
    }

    Ok(matrix)
}

pub fn parse_from_json<T>(path: &str) -> Result<MapMatrix<T>, Box<dyn std::error::Error>>
where
    T: DeserializeOwned + Serialize + PrimInt + Debug + Copy + Hash,
{
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let json_matrix: JsonMatrix = serde_json::from_reader(reader)?;
    let matrix: MapMatrix<T> = serde_json::from_str(&json_matrix.map)?;

    Ok(matrix)
}
