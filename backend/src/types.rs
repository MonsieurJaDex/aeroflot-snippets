use num_traits::PrimInt;
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    hash::{DefaultHasher, Hash, Hasher},
};
use utoipa::{OpenApi, ToSchema};

use crate::error;
use crate::router::__path_get_map;

#[derive(OpenApi)]
#[openapi(
    paths(get_map),
    components(schemas(
        MapMatrix<u8>
    )),
    tags(
        (name="Aeroflot", description="Aeroflot dispatcher management API")
    )
)]
pub struct ApiDoc;

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

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
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
        if initial.is_empty() {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct Point<T>(pub T, pub T)
where
    T: PrimInt + Copy; // Point(x, y)

impl<T> Point<T>
where
    T: PrimInt + Copy,
{
    pub fn new(x: T, y: T) -> Self {
        Self(x, y)
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

#[derive(Debug, Serialize, Clone)]
pub struct Route<T: PrimInt>(Vec<Point<T>>);

impl<T: PrimInt + Hash> Route<T> {
    pub fn new(path: Vec<Point<T>>) -> Self {
        Self(path)
    }

    // method for computing route direction indepenent hash
    pub fn compute_universal_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        if self.0.len() < 2 {
            self.0.hash(&mut hasher);
            return hasher.finish();
        }

        let first = self.0.first().unwrap();
        let last = self.0.last().unwrap();

        let is_forward = (first.0, first.1) <= (last.0, last.1);

        if is_forward {
            first.hash(&mut hasher);
            last.hash(&mut hasher);
        } else {
            last.hash(&mut hasher);
            first.hash(&mut hasher);
        }

        hasher.finish()
    }
}

#[derive(Debug, Serialize)]
pub struct WebResponse<T: PrimInt + Copy> {
    pub width: usize,
    pub height: usize,
    pub matrix: MapMatrix<T>,
}

impl<T: PrimInt + Copy> WebResponse<T> {
    pub fn new(matrix: MapMatrix<T>) -> Result<Self, Box<dyn Error>> {
        if matrix.0.is_empty() {
            return Err(Box::new(error::CommonErrors::InvalidArgument(
                "got empty matrix".to_string(),
            )));
        }

        let width = matrix.0[0].len();
        let height = matrix.0.len();
        Ok(Self {
            matrix,
            width,
            height,
        })
    }
}
