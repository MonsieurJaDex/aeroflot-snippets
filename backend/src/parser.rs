use std::{
    error::Error,
    fs::File,
    io::{BufReader, Write},
};


use crate::{
    error,
    types::{
        map::MapMatrix,
        tmj::JsonMatrix,
        tmj::{TiledLayer, TmjFile},
    },
};

// map parser (tmj -> MapMatrix)
pub fn parse_map(path: &str, save_path: Option<&str>) -> Result<MapMatrix, Box<dyn Error>> {
    let path = path.trim();
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let raw_map: TmjFile = serde_json::from_reader(reader)?;

    let layers: Vec<TiledLayer> = raw_map.layers;

    if layers.is_empty() {
        return Err(Box::new(error::CommonErrors::IncorrectLayer(
            "empty layers".to_string(),
        )));
    }

    let mut matrix: MapMatrix =
        MapMatrix::new(layers[0].width as usize, layers[0].height as usize, 0);

    for layer in layers {
        if layer.width == 0 {
            return Err(Box::new(error::CommonErrors::IncorrectFileContent(
                format!("Zero layer width catched at layer: {}", layer.name),
            )));
        }

        let chunks = layer.data.chunks(layer.width as usize);

        for (matrix_row, chunk_row) in matrix.0.iter_mut().zip(chunks) {
            for (matrix_cell, chunk_cell) in matrix_row.iter_mut().zip(chunk_row) {
                if chunk_cell.eq(&0) {
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

pub fn parse_from_json(path: &str) -> Result<MapMatrix, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let json_matrix: JsonMatrix = serde_json::from_reader(reader)?;
    let matrix: MapMatrix = serde_json::from_str(&json_matrix.map)?;

    Ok(matrix)
}
