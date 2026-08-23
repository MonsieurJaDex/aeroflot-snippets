use std::{
    collections::HashSet,
    error::Error,
    fmt::Debug,
    fs::{self, File},
    hash::Hash,
    io::{BufReader, ErrorKind, Read, Write},
    path::Path,
};

use num_traits::PrimInt;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    error,
    types::{self, JsonMatrix, MapMatrix},
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
    let content: String = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return Err(Box::new(e)),
    };

    let json_data: serde_json::Value = serde_json::from_str(&content)?;

    let layers: Vec<types::TiledLayer<T, u64>> =
        serde_json::from_value(json_data["layers"].clone())?;

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
        let chunks = layer.data.chunks(layer.width as usize);

        for (matrix_row, chunk_row) in matrix.0.iter_mut().zip(chunks) {
            for (matrix_cell, chunk_cell) in matrix_row.iter_mut().zip(chunk_row) {
                if chunk_cell == &T::zero() {
                    continue;
                }
                matrix_cell.clone_from(chunk_cell);
            }
        }
    }

    match save_path {
        Some(p) => {
            let mut fs = File::create(p)?;

            let stringified = serde_json::to_string(&matrix.clone())?;
            let json_matrix = JsonMatrix::new(stringified);

            let stringified = serde_json::to_string(&json_matrix)?;

            _ = fs.write_all(stringified.as_bytes());
        }
        None => (),
    }

    Ok(matrix)
}

pub fn parse_from_json<T>(path: &str) -> Result<MapMatrix<T>, Box<dyn Error>>
where
    T: PrimInt + Serialize + DeserializeOwned,
{
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let json_matrix: JsonMatrix = serde_json::from_reader(reader)?;
    let matrix: MapMatrix<T> = serde_json::from_str(&json_matrix.map)?;

    Ok(matrix)
}
