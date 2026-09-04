use std::hash::{DefaultHasher, Hash, Hasher};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error;

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct MapMatrix(pub Vec<Vec<i64>>);

impl MapMatrix {
    pub fn new(width: usize, height: usize, initial: i64) -> Self {
        let v = vec![vec![initial; width]; height];
        Self(v)
    }

    pub fn empty_new() -> Self {
        Self(vec![vec![]])
    }

    pub fn from_vec(initial: Vec<Vec<i64>>) -> Result<Self, Box<dyn std::error::Error>> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ToSchema, Serialize, Deserialize)]
pub struct Point(pub i64, pub i64);

impl Point {
    pub fn new(x: i64, y: i64) -> Self {
        Self(x, y)
    }
}

#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct Route(Vec<Point>);

impl Route {
    pub fn new(path: Vec<Point>) -> Self {
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
