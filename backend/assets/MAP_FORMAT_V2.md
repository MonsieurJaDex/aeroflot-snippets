# Map format v2 (palette matrix) — CONTRACT

Authoritative contract for `tools/parser` output and `backend/` input. Anything
that emits or consumes a map file must match this document byte-for-byte in
field names. Written before the code; change it only by editing this file first.

## Why

v1 (`{ "map": [[i64]], "road": [i64] }`) stores raw TileD gids, so a cell can
carry exactly one class. The real airport data (`backend/assets/it2026_aero_geodata/`)
has overlays (a taxiway crossing a pedestrian path, a service road crossing a
taxiway) and roads split into speed sections (`service` at 5/8/10/20 km/h,
`taxi` 10, `runway` 300, `pedestrian` 5). v2 keeps `map` a rectangular
`Vec<Vec<i64>>` — the frontend and `GET /api/map` already render that shape —
but the numbers become **palette indices**, and the palette carries the class,
the speed and which masks (road / walk) the entry belongs to.

## File

```json
{
  "format": 2,
  "name": "it2026_aero",
  "cell_size_m": 5.0,
  "origin": { "lat": 55.824695, "lon": 37.612295 },
  "width": 412,
  "height": 372,
  "map": [[0, 0, 3, "..."], "..."],
  "palette": [
    { "id": 1, "type": "pedestrian", "spd_kmh": 5,   "speed_class": "Walk",            "pass": ["walk"] },
    { "id": 8, "type": "building",   "spd_kmh": 0,   "speed_class": null,              "pass": [] },
    { "id": 2, "type": "service",    "spd_kmh": 5,   "speed_class": "VehicleStand",    "pass": ["road", "walk"] },
    { "id": 3, "type": "service",    "spd_kmh": 8,   "speed_class": "VehicleTerminal", "pass": ["road", "walk"] },
    { "id": 4, "type": "service",    "spd_kmh": 10,  "speed_class": "VehicleApron",    "pass": ["road", "walk"] },
    { "id": 5, "type": "service",    "spd_kmh": 20,  "speed_class": "VehicleAirfield", "pass": ["road", "walk"] },
    { "id": 6, "type": "taxi",       "spd_kmh": 10,  "speed_class": "VehicleApron",    "pass": ["road", "walk"] },
    { "id": 7, "type": "runway",     "spd_kmh": 300, "speed_class": "Runway",          "pass": ["road", "walk"] }
  ],
  "layers": { "road": [2, 3, 4, 5, 6, 7], "walk": [1, 2, 3, 4, 5, 6, 7] },
  "objects": [
    { "id": "0", "name": "Терминал", "kind": "terminal", "x": 10, "y": 20, "w": 40, "h": 30, "cx": 30, "cy": 35 }
  ],
  "zones": [
    { "id": "sz-0", "label": "Стоянка 1", "x": 12, "y": 14, "radius_m": 25.0 }
  ],
  "closed_cells": []
}
```

Rules:

- `map[y][x]`, row-major, `height` rows of `width` i64. `0` = empty cell. Any
  non-zero value MUST exist in `palette` (the backend rejects a dangling id).
- `x` grows east, `y` grows **south** (`y` is the row, row 0 is the north edge).
  Cell `(x, y)` covers the square whose north-west corner is
  `origin + (x*cell_size, y*cell_size)` metres.
- `origin` is the **north-west** corner of the grid: `max lat` and `min lon` of
  `airport_boundaries.geojson`. With this origin every emitted coordinate is
  non-negative and a renderer can draw row 0 at the top.
- Geo binding (both directions, in `backend/src/graph.rs`):
  `lat = origin.lat - y*cell/111320`,
  `lon = origin.lon + x*cell/(111320*cos(origin.lat))`.
  `y` is therefore south-positive and `RoadGraph::project/unproject` must use
  the minus sign on `y` (it used `+` when `y` was north-positive).
- `layers.road` — palette ids a vehicle may drive on. `layers.walk` — palette
  ids an engineer may walk on. `walk` is a superset of `road` here, which is
  what stitches the otherwise disconnected pedestrian network to the apron
  (5 pedestrian components at 5 m; the engineer reaches them through taxi /
  service cells).
- `palette[].pass` repeats the layer membership of one entry; `layers` is the
  aggregate the backend consumes and must agree with it.
- `palette[].speed_class` names an existing `backend/src/graph.rs::SpeedClass`
  variant; `Runway` is added by this format (300 km/h, `runway` only). It is
  **null** when `pass` is empty: such an entry is an obstacle (buildings and
  other `objects.geojson` polygons rasterised into `map`), traversable by
  nobody, and the graph emits no node for it.
- Overlap: a cell holds one palette id, chosen by `priority` (parser config,
  default `["runway", "taxi", "service", "pedestrian"]`, ties broken by higher
  `spd_kmh`). The overlapped classes are not lost to routing because the walk
  mask still contains the winning cell — i.e. a taxiway cell under a pedestrian
  path stays walkable and drivable at taxi speed.
- `objects[]` come from `objects.geojson` MultiPolygon bounding boxes in cells;
  `kind` mapping: `Терминал`→`terminal`, `Ангар`→`hangar`, `ВПП`→`runway`,
  `Стоянка спецтехники`→`parking`, `Склад`→`warehouse` (unknown names →
  `other`). `id` is the geojson `fid` as a string when present.
- `zones[]` come from `service_zones.geojson` polygons: centroid in cells,
  `radius_m` = sqrt(area_m2 / pi).
- `closed_cells` is `[]` from the parser; runtime may extend it via the
  `{map_id}.location.json` sidecar.

## v1 compatibility

A file with no `format` (or `format: 1`) keeps loading. Its palette is
synthesised from `road`: every gid in `road` becomes a palette entry
`{ id: gid, type: "legacy", spd_kmh: 10, speed_class: "VehicleApron",
pass: ["road", "walk"] }`, every other non-zero gid observed in `map` becomes
`{ id: gid, type: "legacy", spd_kmh: 10, speed_class: "VehicleApron",
pass: ["walk"] }` (walk-only, for objects/terminals). Behaviour of a v1 map must
be identical to today's.

## Where the origin / objects / zones live

In the map file (v2), not only in the sidecar. `backend/src/location.rs` prefers
the map file's `origin` / `objects` / `zones` / `cell_size_m` and lets
`{map_id}.location.json` override them when it states the same field; the
sidecar keeps the runtime knobs (`sla_secs`, `ata`, `agents`, `stale_after_secs`,
`boarding_delay_secs`, `tool_delay_secs`, `max_accuracy_m`, `closed_cells`).

`max_speed_mps` plausibility in `router/track.rs` must take its ceiling from the
map palette (`max palette spd_kmh`), not from the sidecar's 12 m/s — otherwise a
legal 300 km/h runway movement is dropped as a GPS jump.

## Rust API surface (fixed here so parallel work cannot drift)

`backend/src/types/map.rs` (owner: the types task):

```rust
pub struct Origin { pub lat: f64, pub lon: f64 }

pub struct PaletteEntry {
    pub id: i64,
    #[serde(rename = "type")] pub kind: String,
    pub spd_kmh: i64,
    pub speed_class: Option<String>,
    pub pass: Vec<String>,
}

pub struct MapLayers {
    #[serde(default)] pub road: Vec<i64>,
    #[serde(default)] pub walk: Vec<i64>,
}

pub struct MapObject { pub id: String, pub name: String, pub kind: String,
                       pub x: i64, pub y: i64, pub w: i64, pub h: i64 }

pub struct MapZone { pub id: String, pub label: String, pub x: i64, pub y: i64,
                     pub radius_m: f64 }

pub struct MapDocument {
    pub format: u32,
    pub name: Option<String>,
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
}

impl MapDocument {
    /// Parses a v2 file, or synthesises a v1 document (palette from `road`).
    pub fn from_json(value: serde_json::Value) -> Result<Self, String>;
    /// Road ids as a set; for v1 this is exactly `road`.
    pub fn road_ids(&self) -> std::collections::HashSet<i64>;
    /// Walk ids; for v1 this is exactly `road` (walkability unchanged from today).
    pub fn walk_ids(&self) -> std::collections::HashSet<i64>;
    /// Palette entry by cell value.
    pub fn entry(&self, id: i64) -> Option<&PaletteEntry>;
    /// Fastest legal speed in the palette, km/h (0.0 for v1/empty).
    pub fn max_spd_kmh(&self) -> f64;
    /// Validates: non-zero cells have a palette entry, `layers` agree with
    /// `palette[].pass`, matrix is `height`x`width`. Returns Err with a message.
    pub fn validate(&self) -> Result<(), String>;
}
```

`backend/src/parser.rs` (same owner):

```rust
pub fn load_map_document(path: &std::path::Path) -> anyhow::Result<crate::types::map::MapDocument>;
```

It replaces the old `parse_from_json(&str) -> Result<JsonMatrix, _>`; callers
(`maps.rs`, `location.rs`, `main.rs`) are updated by their own tasks.

`backend/src/graph.rs` (owner of the graph task):

```rust
pub enum SpeedClass { Walk, VehicleStand, VehicleTerminal, VehicleApron, VehicleAirfield, Runway }
impl SpeedClass {
    /// "Walk" | "VehicleStand" | "VehicleTerminal" | "VehicleApron"
    /// | "VehicleAirfield" | "Runway"; unknown names map to VehicleApron.
    pub fn from_name(name: &str) -> SpeedClass;
}
pub fn runway_speed_kmh() -> f64;                       // 300.0
pub fn vehicle_speed_mps(at: Point, class: SpeedClass) -> f64;  // Runway arm added
pub fn max_vehicle_speed_mps() -> f64;                  // includes Runway / palette max
pub fn graph_from_grid(
    matrix: &MapMatrix,
    road_ids: &HashSet<i64>,
    walk_ids: &HashSet<i64>,
    closed: &HashSet<(i64, i64)>,
    cell_size_m: f64,
    origin_lat: f64,
    origin_lon: f64,
    speed_of: &dyn Fn(i64) -> Option<SpeedClass>,   // palette lookup; None = impassable
) -> RoadGraph;
```

`GraphEdge.speed` carries the winning cell's `SpeedClass`, so A* travel time is
per-section, not 10 km/h everywhere. `RoadGraph::max_vehicle_speed_mps` (or the
existing free function) reports the fastest class present, and `track.rs`'s
plausibility ceiling is `max(sidecar.max_speed_mps, map.max_spd_kmh / 3.6)`.

## Frontend hand-off (`frontend/**`, not edited from this branch)

`GET /api/map` now answers a self-describing document. A client must read the
palette; the TileD gids `973/974/975` no longer appear in any matrix.

`frontend/src/lib/apronMap.js` needs exactly three changes:

1. **`ROAD_GIDS` / `BACKEND_ROAD_IDS` (line 26-27) must go.** Walkability is
   `new Set(payload.layers.walk)` and drivability `new Set(payload.layers.road)`,
   because a class is a palette id whose layer membership the map itself states.
   `mask[row][col] = walkIds.has(gid)` at line 203 replaces the `ROAD_GIDS.has`
   test. `TILESETS` (line 3) is TileD presentation only and can stay for the
   `.tmj` fallback.
2. **`loadApronMap()` (line 61-76) must accept the wrapped response.** It
   currently does `const matrix = await apiRequest("/api/map")` and then requires
   `Array.isArray(matrix)`; the endpoint returns
   `{ id, name, matrix, format, width, height, cell_size_m, origin, palette,
   layers, objects, zones, updated_at }`, so that branch never matched and the UI
   silently kept rendering `frontend/public/real_map.tmj` (64x64). Read
   `payload.matrix` plus `payload.palette`/`payload.layers`.
3. **`mapFromMatrix` (line 49) must pass the palette through** instead of
   inventing a single TileD tileset: the renderer needs `palette[id].type` and
   `palette[id].spd_kmh` to colour speed sections, and `objects[]`/`zones[]` to
   label the terminal, hangars and stands.

Until those land, the dispatcher screen still draws the old 64x64 placeholder even
though the backend serves the real 412x372 map.
