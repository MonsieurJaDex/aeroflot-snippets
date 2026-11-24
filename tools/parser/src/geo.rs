//! GeoJSON reading and WGS-84 <-> cell projection for the v2 map builder.
//!
//! The projection is the one `backend/src/graph.rs` uses on the other side:
//! 111_320 metres per degree of latitude and `cos(lat)` for longitude. `y`
//! grows south, `x` grows east, both non-negative, origin at the north-west
//! corner of the airport boundary.

use serde_json::Value;

/// Metres per degree of latitude.
pub const M_PER_DEG: f64 = 111_320.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LonLat {
    pub lon: f64,
    pub lat: f64,
}

#[derive(Debug, Clone)]
pub struct GeoFeature {
    /// Geometry type: `LineString`, `MultiPolygon`, `Polygon`, ...
    pub geom_type: String,
    /// Rings of the geometry: for `LineString` the single line, for polygons
    /// every ring of every polygon (outer and holes alike).
    pub rings: Vec<Vec<LonLat>>,
    pub properties: Value,
}

impl GeoFeature {
    pub fn prop_str(&self, key: &str) -> Option<String> {
        match self.properties.get(key)? {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    pub fn prop_f64(&self, key: &str) -> Option<f64> {
        self.properties.get(key)?.as_f64()
    }

    /// Every point of every ring of the feature.
    pub fn points(&self) -> impl Iterator<Item = LonLat> + '_ {
        self.rings.iter().flatten().copied()
    }

    /// The first (outer) ring, if any.
    pub fn outer(&self) -> Option<&Vec<LonLat>> {
        self.rings.first()
    }
}

fn parse_ring(value: &Value) -> Vec<LonLat> {
    value
        .as_array()
        .map(|points| {
            points
                .iter()
                .filter_map(|p| {
                    let pair = p.as_array()?;
                    Some(LonLat {
                        lon: pair.first()?.as_f64()?,
                        lat: pair.get(1)?.as_f64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Pull line rings out of a geometry's `coordinates`, by geometry type.
fn rings_of(geom_type: &str, coords: &Value, out: &mut Vec<Vec<LonLat>>) {
    match geom_type {
        "LineString" => out.push(parse_ring(coords)),
        "MultiLineString" | "Polygon" => {
            if let Some(parts) = coords.as_array() {
                for part in parts {
                    out.push(parse_ring(part));
                }
            }
        }
        "MultiPolygon" => {
            if let Some(polys) = coords.as_array() {
                for poly in polys {
                    if let Some(rings) = poly.as_array() {
                        for ring in rings {
                            out.push(parse_ring(ring));
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Reads a GeoJSON `FeatureCollection` (or a bare `Feature`) into features.
pub fn read_features(raw: &str) -> Result<Vec<GeoFeature>, String> {
    let value: Value = serde_json::from_str(raw).map_err(|e| format!("geojson parse: {e}"))?;
    let mut features = Vec::new();

    let items: Vec<&Value> = match value.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => value
            .get("features")
            .and_then(Value::as_array)
            .ok_or_else(|| "FeatureCollection without features".to_string())?
            .iter()
            .collect(),
        Some("Feature") => vec![&value],
        other => return Err(format!("unsupported geojson type: {other:?}")),
    };

    for item in items {
        let geometry = match item.get("geometry") {
            Some(g) if g.is_object() => g,
            _ => continue,
        };
        let geom_type = geometry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut rings = Vec::new();
        if let Some(coords) = geometry.get("coordinates") {
            rings_of(&geom_type, coords, &mut rings);
        }
        rings.retain(|ring| !ring.is_empty());
        if rings.is_empty() {
            continue;
        }
        features.push(GeoFeature {
            geom_type,
            rings,
            properties: item
                .get("properties")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new())),
        });
    }

    Ok(features)
}

/// The affine grid binding: origin at the north-west corner, `y` south-positive.
#[derive(Debug, Clone, Copy)]
pub struct Grid {
    pub origin_lat: f64,
    pub origin_lon: f64,
    pub cell_size_m: f64,
    pub width: usize,
    pub height: usize,
}

impl Grid {
    /// Builds a grid covering `features` at `cell_size_m`, anchored at the
    /// north-west corner (`max lat`, `min lon`) of their bounding box.
    pub fn covering(features: &[GeoFeature], cell_size_m: f64, pad_cells: i64) -> Option<Self> {
        let mut min_lat = f64::INFINITY;
        let mut max_lat = f64::NEG_INFINITY;
        let mut min_lon = f64::INFINITY;
        let mut max_lon = f64::NEG_INFINITY;
        for point in features.iter().flat_map(GeoFeature::points) {
            min_lat = min_lat.min(point.lat);
            max_lat = max_lat.max(point.lat);
            min_lon = min_lon.min(point.lon);
            max_lon = max_lon.max(point.lon);
        }
        if !min_lat.is_finite() || !min_lon.is_finite() {
            return None;
        }
        let _ = (min_lat, max_lon);
        let cos = max_lat.to_radians().cos().max(0.2);
        let metres_x = (max_lon - min_lon) * M_PER_DEG * cos;
        let metres_y = (max_lat - min_lat) * M_PER_DEG;
        let pad = pad_cells.max(0) as usize;
        let width = (metres_x / cell_size_m).ceil() as usize + 1 + 2 * pad;
        let height = (metres_y / cell_size_m).ceil() as usize + 1 + 2 * pad;
        Some(Self {
            origin_lat: max_lat,
            origin_lon: min_lon,
            cell_size_m,
            width,
            height,
        })
    }

    /// Continuous cell coordinates of a point (`x` east, `y` south).
    pub fn to_cell(&self, point: LonLat) -> (f64, f64) {
        let cos = self.origin_lat.to_radians().cos().max(0.2);
        let x = (point.lon - self.origin_lon) * M_PER_DEG * cos / self.cell_size_m;
        let y = (self.origin_lat - point.lat) * M_PER_DEG / self.cell_size_m;
        (x, y)
    }

    /// Inverse of [`Grid::to_cell`].
    pub fn to_geo(&self, x: f64, y: f64) -> LonLat {
        let cos = self.origin_lat.to_radians().cos().max(0.2);
        LonLat {
            lon: self.origin_lon + x * self.cell_size_m / (M_PER_DEG * cos),
            lat: self.origin_lat - y * self.cell_size_m / M_PER_DEG,
        }
    }

    pub fn in_bounds(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }
}

/// Area of a lon/lat ring in square metres (shoelace in the cell plane).
pub fn ring_area_m2(grid: &Grid, ring: &[LonLat]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut doubled = 0.0;
    for pair in ring.windows(2) {
        let (x1, y1) = grid.to_cell(pair[0]);
        let (x2, y2) = grid.to_cell(pair[1]);
        doubled += x1 * y2 - x2 * y1;
    }
    let last = ring[ring.len() - 1];
    let first = ring[0];
    let (x1, y1) = grid.to_cell(last);
    let (x2, y2) = grid.to_cell(first);
    doubled += x1 * y2 - x2 * y1;
    (doubled / 2.0).abs() * grid.cell_size_m * grid.cell_size_m
}

/// Bounding box of a ring in cell coordinates: `(min_x, min_y, w, h)`.
pub fn ring_bbox(grid: &Grid, ring: &[LonLat]) -> Option<(i64, i64, i64, i64)> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for point in ring {
        let (x, y) = grid.to_cell(*point);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    if !min_x.is_finite() || !min_y.is_finite() {
        return None;
    }
    let x0 = min_x.floor() as i64;
    let y0 = min_y.floor() as i64;
    let w = (max_x.floor() as i64 - x0 + 1).max(1);
    let h = (max_y.floor() as i64 - y0 + 1).max(1);
    Some((x0, y0, w, h))
}

/// Even-odd point-in-polygon test in the cell plane.
pub fn point_in_ring(grid: &Grid, ring: &[LonLat], x: f64, y: f64) -> bool {
    let mut inside = false;
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = grid.to_cell(ring[i]);
        let (xj, yj) = grid.to_cell(ring[j]);
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}
