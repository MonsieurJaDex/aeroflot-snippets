use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct TmjFile {
    pub layers: Vec<TiledLayer>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonMatrix {
    pub map: String,
}

impl JsonMatrix {
    pub fn new(map: String) -> Self {
        Self { map }
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
