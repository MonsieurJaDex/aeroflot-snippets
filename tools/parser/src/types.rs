use std::{env, io, path::PathBuf};

use serde::{Deserialize, Serialize};

pub struct AppConfig {
    pub config_filepath: PathBuf,
    pub parsed_map_filepath: PathBuf,
}

impl AppConfig {
    pub fn new(cfg_filepath: Option<PathBuf>, map_filepath: Option<PathBuf>) -> io::Result<Self> {
        let cfg = match cfg_filepath {
            Some(p) => p,
            None => {
                let mut default_path = PathBuf::new();
                default_path.push(env::current_dir()?);
                default_path.push("config.json");
                default_path
            }
        };

        let map = match map_filepath {
            Some(p) => p,
            None => {
                let mut default_path = PathBuf::new();
                default_path.push(env::current_dir()?);
                default_path.push("map.json");
                default_path
            }
        };

        Ok(Self {
            config_filepath: cfg,
            parsed_map_filepath: map,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub map_path: String,
    pub roads: Vec<i64>,
}

impl Config {
    pub fn new(map_path: String, roads: Vec<i64>) -> Self {
        Self { map_path, roads }
    }
}

#[derive(Debug, Deserialize)]
pub struct TmjFile {
    pub layers: Vec<TiledLayer>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonMatrix {
    pub map: MapMatrix,
    pub road: Vec<i64>,
}

impl JsonMatrix {
    pub fn new(map: MapMatrix, road: Vec<i64>) -> Self {
        Self { map, road }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TiledLayer {
    pub data: Vec<i64>,
    pub height: i64,
    pub id: i64,
    pub name: String,
    pub opacity: f32,

    #[serde(rename = "type")]
    pub map_type: String,

    pub visible: bool,
    pub width: u32,
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MapMatrix(pub Vec<Vec<i64>>);

impl MapMatrix {
    pub fn new(width: usize, height: usize, initial: i64) -> Self {
        let v = vec![vec![initial; width]; height];
        Self(v)
    }
}
