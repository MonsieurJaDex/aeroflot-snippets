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

/// Raw JSON structure of a v1 map file: `{ "map": [[i64]], "road": [i64] }`.
///
/// Only the loader constructs this now; everything downstream uses
/// [`MapDocument`], whose palette is synthesised from these two fields.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonMatrix {
    /// The map grid.
    pub map: MapMatrix,
    /// TileD gids a vehicle may drive on (v1 walkability == drivability).
    pub road: Vec<i64>,
}

// ---------------------------------------------------------------------------
// Map document (format 2 — see backend/assets/MAP_FORMAT_V2.md)
// ---------------------------------------------------------------------------

use std::collections::{BTreeSet, HashSet};

/// Grid anchor: the north-west corner of cell `(0, 0)`.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, ToSchema)]
pub struct Origin {
    pub lat: f64,
    pub lon: f64,
}

impl Origin {
    /// Default anchor of the placeholder maps.
    #[must_use]
    pub const fn legacy() -> Self {
        Self {
            lat: 55.831,
            lon: 37.636,
        }
    }
}

/// One drivable or walkable cell class.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, ToSchema)]
pub struct PaletteEntry {
    /// Value a cell carries in `map`.
    pub id: i64,
    /// Source class: `service`, `taxi`, `runway`, `pedestrian`, `legacy`, or an
    /// obstacle kind (`terminal`, `hangar`, …).
    #[serde(rename = "type")]
    pub kind: String,
    /// Posted speed in km/h; `0` for an obstacle.
    pub spd_kmh: i64,
    /// Name of the `SpeedClass` variant, `None` for an obstacle.
    pub speed_class: Option<String>,
    /// Layers this class belongs to: `road`, `walk`, or neither (obstacle).
    pub pass: Vec<String>,
}

impl PaletteEntry {
    /// True when a vehicle may drive on this class.
    #[must_use]
    pub fn allows_road(&self) -> bool {
        self.pass.iter().any(|layer| layer == "road")
    }

    /// True when an engineer may walk on this class.
    #[must_use]
    pub fn allows_walk(&self) -> bool {
        self.pass.iter().any(|layer| layer == "walk")
    }

    /// True when nobody may traverse this class (buildings, water, …).
    #[must_use]
    pub fn is_obstacle(&self) -> bool {
        self.pass.is_empty()
    }
}

/// Layer membership of the whole document.
#[derive(Debug, Serialize, Deserialize, Clone, Default, ToSchema)]
pub struct MapLayers {
    /// Palette ids a vehicle may drive on.
    pub road: Vec<i64>,
    /// Palette ids an engineer may walk on.
    pub walk: Vec<i64>,
}

/// A building or other named area, as a cell bounding box.
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct MapObject {
    pub id: String,
    pub name: String,
    /// `terminal`, `hangar`, `runway`, `parking`, `warehouse`, `other`.
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

/// A service zone: a disc in cell coordinates.
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct MapZone {
    pub id: String,
    pub label: String,
    pub x: i64,
    pub y: i64,
    pub radius_m: f64,
}

/// A whole map file, format 2.
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct MapDocument {
    pub format: u32,
    #[serde(default)]
    pub name: String,
    pub cell_size_m: f64,
    pub origin: Origin,
    pub width: usize,
    pub height: usize,
    pub map: MapMatrix,
    pub palette: Vec<PaletteEntry>,
    pub layers: MapLayers,
    #[serde(default)]
    pub objects: Vec<MapObject>,
    #[serde(default)]
    pub zones: Vec<MapZone>,
    #[serde(default)]
    pub closed_cells: Vec<[i64; 2]>,
}

impl MapDocument {
    /// Palette ids a vehicle may drive on, as a set.
    #[must_use]
    pub fn road_ids(&self) -> HashSet<i64> {
        self.layers.road.iter().copied().collect()
    }

    /// Palette ids an engineer may walk on, as a set.
    #[must_use]
    pub fn walk_ids(&self) -> HashSet<i64> {
        self.layers.walk.iter().copied().collect()
    }

    /// Palette entry a cell value refers to.
    #[must_use]
    pub fn entry(&self, id: i64) -> Option<&PaletteEntry> {
        self.palette.iter().find(|entry| entry.id == id)
    }

    /// Fastest legal speed on the map, km/h.
    #[must_use]
    pub fn max_spd_kmh(&self) -> f64 {
        self.palette
            .iter()
            .map(|entry| entry.spd_kmh as f64)
            .fold(0.0, f64::max)
    }

    /// Fastest plausible movement speed in m/s: the palette maximum, never
    /// below the given floor (the sidecar's `max_speed_mps`).
    #[must_use]
    pub fn max_speed_mps(&self, floor: f64) -> f64 {
        (self.max_spd_kmh() / 3.6).max(floor)
    }

    /// Validates every invariant the loader and the graph rely on.
    ///
    /// # Errors
    /// A human-readable message naming the first violated invariant.
    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            return Err("map has a zero dimension".into());
        }
        if self.map.height() != self.height {
            return Err(format!(
                "matrix has {} rows, header says {height}",
                self.map.height(),
                height = self.height
            ));
        }
        if self.map.0.iter().any(|row| row.len() != self.width) {
            return Err(format!("a matrix row is not {width} cells wide", width = self.width));
        }
        if !(self.cell_size_m.is_finite() && self.cell_size_m > 0.0) {
            return Err("cell_size_m must be positive".into());
        }
        if !self.origin.lat.is_finite() || !self.origin.lon.is_finite() {
            return Err("origin must be finite".into());
        }
        let mut seen = BTreeSet::new();
        for entry in &self.palette {
            if !seen.insert(entry.id) {
                return Err(format!("duplicate palette id {}", entry.id));
            }
            if entry.pass.iter().any(|layer| layer != "road" && layer != "walk") {
                return Err(format!("palette {} has an unknown layer", entry.id));
            }
            if entry.is_obstacle() != entry.speed_class.is_none() {
                return Err(format!(
                    "palette {} must name a speed class exactly when it is passable",
                    entry.id
                ));
            }
        }
        for (y, row) in self.map.0.iter().enumerate() {
            for (x, value) in row.iter().enumerate() {
                if *value != 0 && !seen.contains(value) {
                    return Err(format!("cell ({x},{y}) holds dangling palette id {value}"));
                }
            }
        }
        let expected_road: HashSet<i64> = self
            .palette
            .iter()
            .filter(|entry| entry.allows_road())
            .map(|entry| entry.id)
            .collect();
        let expected_walk: HashSet<i64> = self
            .palette
            .iter()
            .filter(|entry| entry.allows_walk())
            .map(|entry| entry.id)
            .collect();
        if self.road_ids() != expected_road {
            return Err("layers.road disagrees with palette[].pass".into());
        }
        if self.walk_ids() != expected_walk {
            return Err("layers.walk disagrees with palette[].pass".into());
        }
        if !expected_road.is_subset(&expected_walk) {
            return Err("every drivable class must be walkable".into());
        }
        for zone in &self.zones {
            if !(zone.radius_m.is_finite() && zone.radius_m > 0.0) {
                return Err(format!("zone {} has a non-positive radius", zone.id));
            }
        }
        for object in &self.objects {
            if object.w <= 0 || object.h <= 0 {
                return Err(format!("object {} has an empty bounding box", object.id));
            }
        }
        Ok(())
    }
}

/// The v1 root object, used when a file carries no `format`.
#[derive(Debug, Deserialize)]
struct LegacyFile {
    map: MapMatrix,
    #[serde(default)]
    road: Vec<i64>,
}

impl MapDocument {
    /// Reads a map file of either generation.
    ///
    /// `format: 2` is validated as-is. A file with no `format` (or `format: 1`)
    /// is the legacy `{map, road}` shape and is upgraded: every gid in `road`
    /// becomes a drivable palette entry, every other gid observed in the matrix
    /// becomes walk-only — the same walkability the old loader implied.
    ///
    /// # Errors
    /// Parse failures and validation failures, as one readable message.
    pub fn from_json(value: serde_json::Value) -> Result<Self, String> {
        let version = value
            .get("format")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1);
        if version >= 2 {
            let doc: Self =
                serde_json::from_value(value).map_err(|error| format!("map v2: {error}"))?;
            doc.validate()?;
            return Ok(doc);
        }

        let legacy: LegacyFile =
            serde_json::from_value(value).map_err(|error| format!("map v1: {error}"))?;
        if legacy.map.width() == 0 || legacy.map.height() == 0 {
            return Err("map v1 has an empty matrix".into());
        }
        let mut observed: BTreeSet<i64> = legacy
            .map
            .0
            .iter()
            .flatten()
            .copied()
            .filter(|value| *value != 0)
            .collect();
        let mut road = legacy.road;
        road.sort_unstable();
        road.dedup();

        let mut palette = Vec::with_capacity(observed.len());
        for id in &road {
            palette.push(PaletteEntry {
                id: *id,
                kind: "legacy".into(),
                spd_kmh: 10,
                speed_class: Some("VehicleApron".into()),
                pass: vec!["road".into(), "walk".into()],
            });
            observed.remove(id);
        }
        for id in observed {
            palette.push(PaletteEntry {
                id,
                kind: "legacy".into(),
                spd_kmh: 10,
                speed_class: Some("VehicleApron".into()),
                pass: vec!["walk".into()],
            });
        }
        palette.sort_by_key(|entry| entry.id);

        let layers = MapLayers {
            road: palette
                .iter()
                .filter(|entry| entry.allows_road())
                .map(|entry| entry.id)
                .collect(),
            walk: palette
                .iter()
                .filter(|entry| entry.allows_walk())
                .map(|entry| entry.id)
                .collect(),
        };
        let doc = Self {
            format: 2,
            name: "legacy".into(),
            cell_size_m: 5.0,
            origin: Origin::legacy(),
            width: legacy.map.width(),
            height: legacy.map.height(),
            map: legacy.map,
            palette,
            layers,
            objects: Vec::new(),
            zones: Vec::new(),
            closed_cells: Vec::new(),
        };
        doc.validate()?;
        Ok(doc)
    }
}
