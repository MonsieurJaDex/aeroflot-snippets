use std::{fs::File, io::BufReader};

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
    let total_cells = width * height;

    let mut matrix = vec![vec![0_i64; width]; height];

    for layer in tiled_map.layers {
        let layer_width = layer.width.max(1);
        let layer_height = layer.height.max(1);
        let layer_size = layer_width * layer_height;

        for (index, tile_id) in layer.data.into_iter().enumerate().take(total_cells) {
            if index >= layer_size {
                break;
            }

            let row = index / width;
            let col = index % width;

            if tile_id != 0 {
                matrix[row][col] = tile_id;
            }
        }
    }

    let mut road = vec![0_i64];
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

#[cfg(test)]
mod tests {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    use super::parse_from_json;

    #[test]
    fn parses_raw_tiled_json_map() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let file = format!("/tmp/tiled_map_{unique}.json");

        fs::write(
            &file,
            r#"{
                "width": 2,
                "height": 2,
                "layers": [
                    {
                        "data": [29, 0, 0, 29],
                        "height": 2,
                        "id": 1,
                        "name": "base",
                        "opacity": 1,
                        "type": "tilelayer",
                        "visible": true,
                        "width": 2,
                        "x": 0,
                        "y": 0
                    }
                ]
            }"#,
        )
        .unwrap();

        let parsed = parse_from_json(&file).unwrap();
        assert_eq!(parsed.map.0.len(), 2);
        assert_eq!(parsed.map.0[0][0], 29);
        assert_eq!(parsed.map.0[0][1], 0);
        assert_eq!(parsed.road, vec![0, 29]);

        fs::remove_file(file).ok();
    }
}
