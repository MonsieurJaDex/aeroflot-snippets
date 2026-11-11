use std::{fs::File, io::BufReader};
use std::collections::HashSet;
use crate::types::map::{JsonMatrix, MapMatrix};

#[derive(serde::Deserialize)]
struct TiledLayer {
    data: Vec<i64>,
    height: usize,
    width: usize,
}

#[derive(serde::Deserialize)]
struct TiledMap {
    width: usize,
    height: usize,
    layers: Vec<TiledLayer>,
}

pub fn parse_from_json(path: &str) -> Result<JsonMatrix, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let value: serde_json::Value = serde_json::from_reader(reader)?;

    if let Ok(json_matrix) = serde_json::from_value::<JsonMatrix>(value.clone()) {
        return Ok(json_matrix);
    }

    let tiled_map: TiledMap = serde_json::from_value(value)?;
    let width = tiled_map.width.max(1);
    let height = tiled_map.height.max(1);

    let mut matrix = vec![vec![0_i64; width]; height];

    for layer in tiled_map.layers {
        let layer_width = layer.width.max(1);
        let layer_height = layer.height.max(1);
        let layer_size = layer_width * layer_height;

        for (index, tile_id) in layer.data.into_iter().enumerate() {
            if index >= layer_size {
                break;
            }

            let row = index / layer_width;
            let col = index % layer_width;
            if row >= height || col >= width {
                continue;
            }

            if tile_id != 0 {
                matrix[row][col] = tile_id;
            }
        }
    }

    let mut road: Vec<i64> = matrix
        .iter()
        .flatten()
        .copied()
        .filter(|v| *v != 0)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    for value in matrix.iter().flatten() {
        if *value != 0 && !road.contains(value) {
            road.push(*value);
        }
    }

    Ok(JsonMatrix {
        map: MapMatrix(matrix),
        road,
    })
}