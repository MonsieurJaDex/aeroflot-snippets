use std::{env, io, path::PathBuf};

use serde::{Deserialize, Serialize};

pub struct AppConfig {
    pub config_filepath: PathBuf,
    pub parsed_map_filepath: PathBuf,
}

/// Config lookup order: `$PARSER_CONFIG`, `./config.json`, then the copy next
/// to `src/` (so the tool can be run from the repository root).
fn config_path() -> io::Result<PathBuf> {
    if let Ok(explicit) = env::var("PARSER_CONFIG") {
        return Ok(PathBuf::from(explicit));
    }
    let cwd = env::current_dir()?.join("config.json");
    if cwd.is_file() {
        return Ok(cwd);
    }
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.json"))
}

impl AppConfig {
    pub fn new(cfg_filepath: Option<PathBuf>, map_filepath: Option<PathBuf>) -> io::Result<Self> {
        let cfg = match cfg_filepath {
            Some(p) => p,
            None => config_path()?,
        };

        let map = match map_filepath {
            Some(p) => p,
            None => {
                let mut default_path = PathBuf::new();
                default_path.push(env::current_dir()?);
                default_path.push("map.json");
                default_path
            }
        };

        Ok(Self {
            config_filepath: cfg,
            parsed_map_filepath: map,
        })
    }
}

fn default_cell() -> f64 {
    5.0
}

fn default_priority() -> Vec<String> {
    ["runway", "taxi", "service", "pedestrian"]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

fn default_stitch() -> i64 {
    1
}

fn default_sla() -> u64 {
    900
}
fn default_alert() -> u64 {
    300
}
fn default_boarding() -> f64 {
    10.0
}
fn default_tool() -> f64 {
    30.0
}
fn default_accuracy() -> f64 {
    80.0
}
fn default_stale() -> i64 {
    45
}

/// Parser configuration. Both the legacy TMJ keys (`map_path`, `roads`) and the
/// GeoJSON keys are accepted; `source` selects the mode.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    /// `"tmj"` (default) or `"geojson"`.
    #[serde(default)]
    pub source: String,
    /// Legacy TMJ input.
    #[serde(default)]
    pub map_path: String,
    /// Legacy TMJ walkable gids; also the road palette of a TMJ output.
    #[serde(default)]
    pub roads: Vec<i64>,

    /// Directory holding the GeoJSON files, relative to the process CWD.
    #[serde(default = "default_input_dir")]
    pub input_dir: String,
    #[serde(default = "default_roads_file")]
    pub roads_file: String,
    #[serde(default = "default_objects_file")]
    pub objects_file: String,
    #[serde(default = "default_zones_file")]
    pub zones_file: String,
    #[serde(default = "default_boundary_file")]
    pub boundary_file: String,

    #[serde(default = "default_cell")]
    pub cell_size_m: f64,
    /// Class priority, highest first, for cells claimed by several classes.
    #[serde(default = "default_priority")]
    pub priority: Vec<String>,
    /// Dilation radius (cells) used to stitch isolated pedestrian components
    /// onto the road network.
    #[serde(default = "default_stitch")]
    pub stitch_radius_cells: i64,
    /// Map id, also used for the output file stem.
    #[serde(default = "default_name")]
    pub name: String,
    /// Where the v2 map is written.
    #[serde(default)]
    pub out: String,
    /// Where the sidecar is written.
    #[serde(default)]
    pub location_out: String,
    /// Optional existing sidecar to carry runtime knobs (ata, agents) from.
    #[serde(default)]
    pub runtime_from: String,

    #[serde(default = "default_sla")]
    pub sla_secs: u64,
    #[serde(default = "default_alert")]
    pub alert_before_secs: u64,
    #[serde(default = "default_boarding")]
    pub boarding_delay_secs: f64,
    #[serde(default = "default_tool")]
    pub tool_delay_secs: f64,
    #[serde(default = "default_accuracy")]
    pub max_accuracy_m: f64,
    #[serde(default = "default_stale")]
    pub stale_after_secs: i64,
}

fn default_input_dir() -> String {
    "backend/assets/it2026_aero_geodata".to_string()
}
fn default_roads_file() -> String {
    "roads.geojson".to_string()
}
fn default_objects_file() -> String {
    "objects.geojson".to_string()
}
fn default_zones_file() -> String {
    "service_zones.geojson".to_string()
}
fn default_boundary_file() -> String {
    "airport_boundaries.geojson".to_string()
}
fn default_name() -> String {
    "it2026_aero".to_string()
}

/// The repository root, i.e. the directory that contains `tools/parser`.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

/// Resolves a configured path: absolute stays as-is, relative is taken from
/// the process CWD when it exists there and from the repository root
/// otherwise. That makes `config.json` usable from any working directory.
pub fn resolve(raw: &str) -> PathBuf {
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() || candidate.exists() {
        return candidate;
    }
    repo_root().join(&candidate)
}

impl Config {
    pub fn new(map_path: String, roads: Vec<i64>) -> Self {
        Self {
            source: "tmj".to_string(),
            map_path,
            roads,
            input_dir: default_input_dir(),
            roads_file: default_roads_file(),
            objects_file: default_objects_file(),
            zones_file: default_zones_file(),
            boundary_file: default_boundary_file(),
            cell_size_m: default_cell(),
            priority: default_priority(),
            stitch_radius_cells: default_stitch(),
            name: default_name(),
            out: String::new(),
            location_out: String::new(),
            runtime_from: String::new(),
            sla_secs: default_sla(),
            alert_before_secs: default_alert(),
            boarding_delay_secs: default_boarding(),
            tool_delay_secs: default_tool(),
            max_accuracy_m: default_accuracy(),
            stale_after_secs: default_stale(),
        }
    }

    /// Output path for the v2 map.
    pub fn out_path(&self) -> PathBuf {
        if self.out.is_empty() {
            repo_root().join(format!("backend/assets/{}.json", self.name))
        } else {
            resolve(&self.out)
        }
    }

    /// Output path for the sidecar.
    pub fn location_path(&self) -> PathBuf {
        if self.location_out.is_empty() {
            repo_root().join(format!("backend/assets/{}.location.json", self.name))
        } else {
            resolve(&self.location_out)
        }
    }

    /// Input directory holding the GeoJSON files.
    pub fn input_path(&self) -> PathBuf {
        resolve(&self.input_dir)
    }

    /// Optional sidecar to carry runtime knobs from.
    pub fn runtime_path(&self) -> Option<PathBuf> {
        if self.runtime_from.is_empty() {
            None
        } else {
            Some(resolve(&self.runtime_from))
        }
    }
}

// ---------------------------------------------------------------------------
// Tiled input
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TmjFile {
    pub layers: Vec<TiledLayer>,
}

/// One TileD layer. Only tile layers carry `data`; object and image groups do
/// not, so those fields are optional and such layers are skipped when
/// flattening.
#[derive(Debug, Deserialize)]
pub struct TiledLayer {
    #[serde(default)]
    pub data: Vec<i64>,
    #[serde(default)]
    pub height: i64,
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub opacity: f32,

    #[serde(rename = "type", default)]
    pub map_type: String,

    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub x: i64,
    #[serde(default)]
    pub y: i64,
}

impl TiledLayer {
    /// True for a layer that contributes cells to the matrix.
    pub fn is_tile_layer(&self) -> bool {
        self.map_type.is_empty() || self.map_type == "tilelayer"
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MapMatrix(pub Vec<Vec<i64>>);

impl MapMatrix {
    pub fn new(width: usize, height: usize, initial: i64) -> Self {
        let v = vec![vec![initial; width]; height];
        Self(v)
    }

    pub fn height(&self) -> usize {
        self.0.len()
    }

    pub fn width(&self) -> usize {
        self.0.first().map_or(0, Vec::len)
    }
}

// ---------------------------------------------------------------------------
// v2 output — must match backend/assets/MAP_FORMAT_V2.md exactly
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Origin {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PaletteEntry {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub spd_kmh: i64,
    pub speed_class: Option<String>,
    pub pass: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MapLayers {
    pub road: Vec<i64>,
    pub walk: Vec<i64>,
}

/// Every class that claimed one cell, when more than one did.
///
/// `map[y][x]` holds a single id, so a cell where a taxiway crosses a service
/// road at a different speed can only show the winner. This table keeps the
/// losers: the first id is the one in `map` (priority winner), the rest are the
/// co-located classes in priority order. Two *speed sections* overlapping are
/// therefore both recoverable, which the layer masks alone cannot do.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CellOverlay {
    pub x: i64,
    pub y: i64,
    /// The palette id stored in `map`.
    pub id: i64,
    /// All ids that claimed the cell, winner first.
    pub ids: Vec<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MapObject {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MapZone {
    pub id: String,
    pub label: String,
    pub x: i64,
    pub y: i64,
    pub radius_m: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MapDocument {
    pub format: u32,
    pub name: String,
    pub cell_size_m: f64,
    pub origin: Origin,
    pub width: usize,
    pub height: usize,
    pub map: MapMatrix,
    pub palette: Vec<PaletteEntry>,
    pub layers: MapLayers,
    pub objects: Vec<MapObject>,
    pub zones: Vec<MapZone>,
    pub closed_cells: Vec<[i64; 2]>,
    /// Cells claimed by more than one class. Empty when nothing overlaps.
    #[serde(default)]
    pub overlays: Vec<CellOverlay>,
}

// ---------------------------------------------------------------------------
// Sidecar
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SidecarZone {
    pub id: String,
    pub x: i64,
    pub y: i64,
    pub radius_m: f64,
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SidecarObject {
    pub id: String,
    pub kind: String,
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Sidecar {
    #[serde(default)]
    pub cell_size_m: f64,
    #[serde(default)]
    pub origin_lat: f64,
    #[serde(default)]
    pub origin_lon: f64,
    #[serde(default)]
    pub sla_secs: u64,
    #[serde(default)]
    pub alert_before_secs: u64,
    #[serde(default)]
    pub boarding_delay_secs: f64,
    #[serde(default)]
    pub tool_delay_secs: f64,
    #[serde(default)]
    pub max_accuracy_m: f64,
    #[serde(default)]
    pub max_speed_mps: f64,
    #[serde(default)]
    pub stale_after_secs: i64,
    #[serde(default)]
    pub zones: Vec<SidecarZone>,
    #[serde(default)]
    pub objects: Vec<SidecarObject>,
    #[serde(default)]
    pub ata: Vec<serde_json::Value>,
    #[serde(default)]
    pub agents: Vec<serde_json::Value>,
    #[serde(default)]
    pub closed_cells: Vec<[i64; 2]>,
}

impl MapDocument {
    /// Road palette ids.
    pub fn road_ids(&self) -> std::collections::HashSet<i64> {
        self.layers.road.iter().copied().collect()
    }

    /// Walk palette ids.
    pub fn walk_ids(&self) -> std::collections::HashSet<i64> {
        self.layers.walk.iter().copied().collect()
    }

    /// Fastest legal speed in the palette, km/h.
    pub fn max_spd_kmh(&self) -> f64 {
        self.palette
            .iter()
            .map(|e| e.spd_kmh as f64)
            .fold(0.0, f64::max)
    }

    /// Checks every invariant the backend relies on. An invalid document must
    /// never reach disk.
    pub fn validate(&self) -> Result<(), String> {
        if self.format != 2 {
            return Err(format!("expected format 2, got {}", self.format));
        }
        if self.width == 0 || self.height == 0 {
            return Err("map has a zero dimension".into());
        }
        if self.map.0.len() != self.height {
            return Err(format!(
                "matrix has {} rows, header says height {}",
                self.map.0.len(),
                self.height
            ));
        }
        for (row, values) in self.map.0.iter().enumerate() {
            if values.len() != self.width {
                return Err(format!(
                    "row {row} has {} cells, header says width {}",
                    values.len(),
                    self.width
                ));
            }
        }
        if !(self.cell_size_m.is_finite() && self.cell_size_m > 0.0) {
            return Err("cell_size_m must be positive".into());
        }
        if !self.origin.lat.is_finite() || !self.origin.lon.is_finite() {
            return Err("origin must be finite".into());
        }
        let mut seen = std::collections::HashSet::new();
        for entry in &self.palette {
            if !seen.insert(entry.id) {
                return Err(format!("duplicate palette id {}", entry.id));
            }
            for pass in &entry.pass {
                if pass != "road" && pass != "walk" {
                    return Err(format!("palette {} has unknown pass {pass:?}", entry.id));
                }
            }
            if entry.pass.is_empty() && entry.speed_class.is_some() {
                return Err(format!(
                    "palette {} is impassable but names a speed class",
                    entry.id
                ));
            }
            if !entry.pass.is_empty() && entry.speed_class.is_none() {
                return Err(format!(
                    "palette {} is passable but names no speed class",
                    entry.id
                ));
            }
        }
        for (y, values) in self.map.0.iter().enumerate() {
            for (x, value) in values.iter().enumerate() {
                if *value != 0 && !seen.contains(value) {
                    return Err(format!("cell ({x},{y}) holds dangling palette id {value}"));
                }
            }
        }
        for id in self.layers.road.iter().chain(self.layers.walk.iter()) {
            if !seen.contains(id) {
                return Err(format!("layers name unknown palette id {id}"));
            }
        }
        let mut expected_road = std::collections::HashSet::new();
        let mut expected_walk = std::collections::HashSet::new();
        for entry in &self.palette {
            if entry.pass.iter().any(|p| p == "road") {
                expected_road.insert(entry.id);
            }
            if entry.pass.iter().any(|p| p == "walk") {
                expected_walk.insert(entry.id);
            }
        }
        if self.road_ids() != expected_road {
            return Err("layers.road disagrees with palette[].pass".into());
        }
        if self.walk_ids() != expected_walk {
            return Err("layers.walk disagrees with palette[].pass".into());
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
        // Overlays must agree with the matrix: each names the cell it describes,
        // starts with the id actually stored there, and mentions only known ids.
        for overlay in &self.overlays {
            if overlay.x < 0
                || overlay.y < 0
                || overlay.x as usize >= self.width
                || overlay.y as usize >= self.height
            {
                return Err(format!(
                    "overlay ({},{}) is outside the {}x{} grid",
                    overlay.x, overlay.y, self.width, self.height
                ));
            }
            let stored = self.map.0[overlay.y as usize][overlay.x as usize];
            if overlay.id != stored {
                return Err(format!(
                    "overlay ({},{}) claims id {} but the map holds {}",
                    overlay.x, overlay.y, overlay.id, stored
                ));
            }
            if overlay.ids.first() != Some(&stored) {
                return Err(format!(
                    "overlay ({},{}) does not start with the stored id {}",
                    overlay.x, overlay.y, stored
                ));
            }
            if overlay.ids.len() < 2 {
                return Err(format!(
                    "overlay ({},{}) is not an overlap: only one id",
                    overlay.x, overlay.y
                ));
            }
            for id in &overlay.ids {
                if !seen.contains(id) {
                    return Err(format!(
                        "overlay ({},{}) names unknown palette id {id}",
                        overlay.x, overlay.y
                    ));
                }
            }
        }
        Ok(())
    }
}

/// The v1 root object, used by [`MapDocument::from_json`] when a file carries
/// no `format` field.
#[derive(Debug, Deserialize)]
struct V1File {
    map: MapMatrix,
    #[serde(default)]
    road: Vec<i64>,
}

impl MapDocument {
    /// Reads a map file of either generation.
    ///
    /// A document with `format: 2` is validated as-is. A file with no `format`
    /// (or `format: 1`) is the legacy `{map, road}` shape and is upgraded: each
    /// gid in `road` becomes a drivable palette entry, every other gid observed
    /// in the matrix becomes a walk-only entry, exactly the behaviour of the
    /// old loader.
    pub fn from_json(value: serde_json::Value) -> Result<Self, String> {
        let version = value
            .get("format")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1);
        if version >= 2 {
            let doc: MapDocument =
                serde_json::from_value(value).map_err(|e| format!("v2 map: {e}"))?;
            doc.validate()?;
            return Ok(doc);
        }

        let legacy: V1File =
            serde_json::from_value(value).map_err(|e| format!("v1 map: {e}"))?;
        if legacy.map.width() == 0 || legacy.map.height() == 0 {
            return Err("v1 map has an empty matrix".into());
        }
        let mut observed: std::collections::BTreeSet<i64> = legacy
            .map
            .0
            .iter()
            .flatten()
            .copied()
            .filter(|v| *v != 0)
            .collect();
        let mut road = legacy.road.clone();
        road.sort_unstable();
        road.dedup();
        let mut palette = Vec::new();
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
        palette.sort_by_key(|e| e.id);
        let layers = MapLayers {
            road: palette
                .iter()
                .filter(|e| e.pass.iter().any(|p| p == "road"))
                .map(|e| e.id)
                .collect(),
            walk: palette
                .iter()
                .filter(|e| e.pass.iter().any(|p| p == "walk"))
                .map(|e| e.id)
                .collect(),
        };
        let doc = Self {
            format: 2,
            name: "legacy".into(),
            cell_size_m: 5.0,
            origin: Origin {
                lat: 55.831,
                lon: 37.636,
            },
            width: legacy.map.width(),
            height: legacy.map.height(),
            map: legacy.map,
            palette,
            layers,
            objects: Vec::new(),
            zones: Vec::new(),
            closed_cells: Vec::new(),
            overlays: Vec::new(),
        };
        doc.validate()?;
        Ok(doc)
    }

    /// Palette entry for a cell value.
    pub fn entry(&self, id: i64) -> Option<&PaletteEntry> {
        self.palette.iter().find(|e| e.id == id)
    }
}
