use std::error::Error;

use crate::error;

pub type MapMatrix<T> = Vec<Vec<T>>;

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
