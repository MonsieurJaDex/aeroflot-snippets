use std::{
    fs::File,
    io::{self, BufReader},
};

use crate::types::{AppConfig, Config, JsonMatrix, MapMatrix, TiledLayer, TmjFile};

pub fn init_config_file(app_config: &AppConfig) -> io::Result<()> {
    let file = File::create(&app_config.config_filepath)?;

    let initial_cfg = Config::new("".to_string(), vec![]);
    serde_json::to_writer_pretty(file, &initial_cfg)?;

    Ok(())
}

pub fn parse_map(app_config: &AppConfig) -> io::Result<()> {
    let cfg_file = File::open(&app_config.config_filepath)?;
    let reader = BufReader::new(cfg_file);
    let config: Config = serde_json::from_reader(reader)?;

    let map_file = File::open(&config.map_path)?;
    let reader = BufReader::new(map_file);
    let raw_map: TmjFile = serde_json::from_reader(reader)?;

    let layers: Vec<TiledLayer> = raw_map.layers;

    if layers.is_empty() {
        return Err(io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provided layers were empty",
        ));
    }

    let mut matrix: MapMatrix =
        MapMatrix::new(layers[0].width as usize, layers[0].height as usize, 0);

    for layer in layers {
        if layer.width == 0 {
            return Err(io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "layer width is zero",
            ));
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

    let json_matrix = JsonMatrix::new(matrix, config.roads);

    let new_file = File::create(&app_config.parsed_map_filepath)?;
    serde_json::to_writer_pretty(new_file, &json_matrix)?;

    Ok(())
}
