use std::{
    fmt::format,
    hash::{DefaultHasher, Hash, Hasher},
};

use anyhow::{Ok, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::from_str;
use utoipa::ToSchema;

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

    pub fn from_vec(initial: Vec<Vec<i64>>) -> anyhow::Result<Self> {
        if initial.is_empty() {
            return Err(anyhow!("initial vector is empty".to_string()));
        }

        let required_row_len = initial[0].len();
        for row in &initial {
            if row.len().ne(&required_row_len) {
                return Err(anyhow!("invalid vector row length".to_string()));
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

    pub fn as_value(&self) -> String {
        format!("{},{}", self.0, self.1)
    }

    pub fn from_value(value: String) -> anyhow::Result<Self> {
        let splited = value
            .trim()
            .split(",")
            .map(|v| from_str::<i64>(v))
            .collect::<Result<Vec<i64>, serde_json::Error>>()?;
        if splited.len() != 2 {
            return Err(anyhow!(
                "got incorrect value format that cannot be parsed to Point"
            ));
        }
        Ok(Self(splited[0], splited[1]))
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

    pub fn get_vec(&self) -> &Vec<Point> {
        &self.0
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonMatrix {
    pub map: MapMatrix,
    pub road: Vec<i64>,
}
