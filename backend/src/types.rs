use num_traits::PrimInt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::error::Error;

use crate::error;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MapMatrix<T>(pub Vec<Vec<T>>)
where
    T: PrimInt;

impl<T> MapMatrix<T>
where
    T: PrimInt,
{
    pub fn new(width: usize, height: usize, initial: T) -> Self {
        let v = vec![vec![initial; width]; height];
        Self(v)
    }

    pub fn empty_new() -> Self {
        Self(vec![vec![]])
    }

    pub fn from_vec(initial: Vec<Vec<T>>) -> Result<Self, Box<dyn Error>> {
        if initial.len() == 0 {
            return Err(Box::new(error::CommonErrors::InvalidArgument(
                "initial vector is empty".to_string(),
            )));
        }

        let required_row_len = initial[0].len();
        for row in &initial {
            if row.len().ne(&required_row_len) {
                return Err(Box::new(error::CommonErrors::InvalidArgument(
                    "invalid vector row length".to_string(),
                )));
            }
        }

        Ok(Self(initial))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point<T>(pub T, pub T); // Point(x, y)

impl<T> Point<T> {
    pub fn new(x: T, y: T) -> Self {
        Self(x, y)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum EngineerType {
    Electronic,
    Mechanical,
}

pub struct Engineers<T> {
    pub points: Vec<Point<T>>,
    pub types: Vec<EngineerType>,
    pub tile: Vec<T>,
}

impl<T> Engineers<T> {
    fn new(
        _points: Vec<Point<T>>,
        _types: Vec<EngineerType>,
        _tile: Vec<T>,
    ) -> Result<Self, Box<dyn Error>> {
        let len_sum = _points.len() + _types.len() + _tile.len();
        if len_sum == 0 || len_sum % 3 != 0 {
            return Err(Box::new(error::CommonErrors::InvalidArgument(
                "arguments should have equal length and more than a zero".to_string(),
            )));
        }

        Ok(Self {
            points: _points,
            types: _types,
            tile: _tile,
        })
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
