use std::hash::{DefaultHasher, Hash, Hasher};

use num_traits::PrimInt;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error;

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

    pub fn from_vec(initial: Vec<Vec<T>>) -> Result<Self, Box<dyn std::error::Error>> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ToSchema)]
pub struct Point<T>
where
    T: PrimInt + Copy,
{
    pub x: T,
    pub y: T,
}

impl<T> Point<T>
where
    T: PrimInt + Copy,
{
    pub fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl<T> Serialize for Point<T>
where
    T: PrimInt + Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        (self.x, self.y).serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for Point<T>
where
    T: PrimInt + Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (x, y) = <(T, T)>::deserialize(deserializer)?;
        Ok(Self { x, y })
    }
}

#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct Route<T: PrimInt>(Vec<Point<T>>);

impl<T: PrimInt + Hash> Route<T> {
    pub fn new(path: Vec<Point<T>>) -> Self {
        Self(path)
    }

    pub fn len(&self) -> usize {
        self.0.len() - 1
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

        let is_forward = (first.x, first.y) <= (last.x, last.y);

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
