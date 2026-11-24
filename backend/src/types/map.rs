//! Airport map primitives: matrix, points and computed routes.
//!
//! The map is a rectangular grid; each cell is an `i64` where `0` usually
//! denotes a free cell and any non-zero value denotes an obstacle. The
//! [`MapMatrix`] and [`Point`] types are exported to the OpenAPI schema.

use std::{fmt, str::FromStr};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ---------------------------------------------------------------------------
// Map matrix
// ---------------------------------------------------------------------------

/// Dense two-dimensional grid describing an airport map.
///
/// Each cell is an integer: `0` means the cell is walkable, any other value is an obstacle.
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[schema(
    description = "Rectangular airport grid. 0 — проходимо, любое другое значение — препятствие.",
    example = json!([[0, 1, 0], [0, 0, 0], [1, 0, 0]])
)]
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

    /// Returns the number of points in the route.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
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
