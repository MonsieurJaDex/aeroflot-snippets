//! Airport map primitives: matrix, points and computed routes.
//!
//! The map is a rectangular grid; each cell is an `i64` where `0` usually
//! denotes a free cell and any non-zero value denotes an obstacle. The
//! [`MapMatrix`] and [`Point`] types are exported to the OpenAPI schema.

use std::{
    fmt,
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ---------------------------------------------------------------------------
// Map matrix
// ---------------------------------------------------------------------------

/// Dense two-dimensional grid describing an airport map.
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct MapMatrix(pub Vec<Vec<i64>>);

impl MapMatrix {
    /// Number of rows (height) of the map.
    #[must_use]
    pub fn height(&self) -> usize {
        self.0.len()
    }

    /// Number of columns (width) of the map.
    ///
    /// Returns `0` for an empty matrix.
    #[must_use]
    pub fn width(&self) -> usize {
        self.0.first().map_or(0, Vec::len)
    }

    /// Returns the cell value at `(x, y)` if `(x, y)` is inside the matrix.
    ///
    /// Note: `x` indexes the column, `y` indexes the row.
    #[must_use]
    pub fn get(&self, x: i64, y: i64) -> Option<i64> {
        if x < 0 || y < 0 {
            return None;
        }
        let (x, y) = (usize::try_from(x).ok()?, usize::try_from(y).ok()?);
        self.0.get(y).and_then(|row| row.get(x)).copied()
    }

    /// Returns `true` if the given point lies inside the matrix bounds.
    #[must_use]
    pub fn in_bounds(&self, point: Point) -> bool {
        point.0 >= 0
            && point.1 >= 0
            && (point.0 as usize) < self.width()
            && (point.1 as usize) < self.height()
    }
}

// ---------------------------------------------------------------------------
// Point
// ---------------------------------------------------------------------------

/// A coordinate on the airport map.
///
/// Serialised as a JSON array `[x, y]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ToSchema, Serialize, Deserialize)]
pub struct Point(pub i64, pub i64);

impl Point {
    /// Creates a new point.
    #[must_use]
    pub const fn new(x: i64, y: i64) -> Self {
        Self(x, y)
    }

    /// Returns the point as a `"x,y"` string suitable for use as a Redis key.
    #[must_use]
    pub fn as_value(&self) -> String {
        format!("{},{}", self.0, self.1)
    }

    /// Parses a point from the canonical `"x,y"` string representation.
    ///
    /// Whitespace around each coordinate is trimmed. Extra `,` characters
    /// cause a parse error.
    pub fn from_value(value: impl AsRef<str>) -> anyhow::Result<Self> {
        let value = value.as_ref().trim();
        let (x_str, y_str) = value
            .split_once(',')
            .ok_or_else(|| anyhow!("expected 'x,y' format, got: {value:?}"))?;

        let x = x_str
            .trim()
            .parse::<i64>()
            .context("invalid x coordinate")?;
        let y = y_str
            .trim()
            .parse::<i64>()
            .context("invalid y coordinate")?;

        Ok(Self(x, y))
    }

    /// Manhattan distance to another point.
    #[must_use]
    pub fn manhattan_distance(&self, other: &Self) -> u64 {
        self.0.abs_diff(other.0) + self.1.abs_diff(other.1)
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.0, self.1)
    }
}

impl FromStr for Point {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_value(s)
    }
}

// ---------------------------------------------------------------------------
// Route
// ---------------------------------------------------------------------------

/// An ordered sequence of points forming a path on the map.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, ToSchema)]
pub struct Route(Vec<Point>);

impl Route {
    /// Creates a route from the given sequence of points.
    #[must_use]
    pub fn new(path: Vec<Point>) -> Self {
        Self(path)
    }

    /// Borrows the underlying path as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[Point] {
        &self.0
    }

    /// Returns the number of points in the route.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the route contains no points.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns a **direction-independent** hash of the route.
    ///
    /// Only the first and last points are hashed, so `A → B` and `B → A`
    /// produce the same key. Useful as a cache key.
    #[must_use]
    pub fn compute_universal_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        if self.0.len() < 2 {
            self.0.hash(&mut hasher);
            return hasher.finish();
        }

        // `len >= 2` — both `first()` and `last()` are `Some`.
        let first = self.0.first().unwrap();
        let last = self.0.last().unwrap();

        if (first.0, first.1) <= (last.0, last.1) {
            first.hash(&mut hasher);
            last.hash(&mut hasher);
        } else {
            last.hash(&mut hasher);
            first.hash(&mut hasher);
        }

        hasher.finish()
    }
}

// ---------------------------------------------------------------------------
// JSON input
// ---------------------------------------------------------------------------

/// Raw JSON structure used to parse the airport map from disk.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonMatrix {
    /// The map grid.
    pub map: MapMatrix,
    /// Flattened list of walkable road cells.
    pub road: Vec<i64>,
}