use num_traits::PrimInt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct TmjDto<T: PrimInt, U: PrimInt> {
    pub layers: Vec<TiledLayer<T, U>>,
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
pub struct TiledLayer<T, U> {
    pub data: Vec<T>,
    pub height: U,
    pub id: U,
    pub name: String,
    pub opacity: f32,

    #[serde(rename = "type")]
    pub map_type: String,

    pub visible: bool,
    pub width: u32,
    pub x: U,
    pub y: U,
}
