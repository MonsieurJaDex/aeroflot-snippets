# parser

Builds the airport map matrix the backend loads. Two inputs, one output
generation:

| input | output | command |
|---|---|---|
| TileD `.tmj` | v1 `{ "map": [[i64]], "road": [i64] }` (unchanged) | `parser parse` with `"source": "tmj"` |
| v1 file | v2 palette document | `parser upgrade` |
| airport GeoJSON | v2 palette document + sidecar | `parser parse` with `"source": "geojson"` |

The v2 schema is specified in [`backend/assets/MAP_FORMAT_V2.md`](../../backend/assets/MAP_FORMAT_V2.md).
That document is the contract; this tool and the backend both follow it.

## Running

From the repository root:

```sh
cargo run --manifest-path tools/parser/Cargo.toml -- init      # write a config skeleton
cargo run --manifest-path tools/parser/Cargo.toml -- parse     # build the map
cargo run --manifest-path tools/parser/Cargo.toml -- upgrade   # v1 file -> v2 document
cargo test --manifest-path tools/parser/Cargo.toml             # acceptance tests
```

`config.json` is looked up in `$PARSER_CONFIG`, then `./config.json`, then next
to `src/`. Relative paths inside it are resolved against the process CWD when
they exist there, otherwise against the repository root, so the same config
works from any directory.

## GeoJSON mode

```json
{
  "source": "geojson",
  "input_dir": "backend/assets/it2026_aero_geodata",
  "roads_file": "roads.geojson",
  "objects_file": "objects.geojson",
  "zones_file": "service_zones.geojson",
  "boundary_file": "airport_boundaries.geojson",
  "cell_size_m": 5.0,
  "priority": ["runway", "taxi", "service", "pedestrian"],
  "stitch_radius_cells": 1,
  "name": "it2026_aero",
  "out": "backend/assets/it2026_aero.json",
  "location_out": "backend/assets/it2026_aero.location.json",
  "runtime_from": "backend/assets/map2.location.json"
}
```

- `cell_size_m` — grid resolution. 5 m keeps every speed section and every
  crossing distinct; 10 m halves the file and merges some crossings.
- `priority` — which class wins a cell claimed by several, highest first. Ties
  inside one class are broken by the higher `spd_kmh`. The losing classes are
  not lost from routing: a cell is drivable if its winner is drivable and
  walkable if its winner is walkable, and `layers.walk` is a superset of
  `layers.road`.
- `stitch_radius_cells` — if the walk mask is still in several components after
  rasterising, the builder paints a shortest 4-neighbour bridge from each
  road-less component towards the road network. With the current data at 5 m no
  bridge is needed and 0 cells are painted; the counter is printed so a future
  dataset cannot silently disconnect the engineer.
- `runtime_from` — an existing sidecar whose runtime knobs (`ata`, `agents`)
  are carried into the new one.

### What the builder does

1. Grid from `airport_boundaries.geojson`: origin at the north-west corner
   (`max lat`, `min lon`), so every index is non-negative and row 0 is the
   north edge. `y` grows south.
2. Every road LineString is rasterised *supercover*: a segment is sampled every
   half cell and a diagonal step also paints the corner cell, otherwise the
   4-neighbour routes fall apart into hundreds of one-cell fragments.
3. One palette entry per distinct `(type, spd_kmh)` — that is what preserves
   the speed sections (`service` at 5/8/10/20 km/h, `taxi` 10, `runway` 300,
   `pedestrian` 5).
4. Overlapping classes are resolved by `priority`; the count of cells that had
   two or more candidates is printed (`overlap cells`).
5. Object polygons are rasterised as impassable palette entries (so a route
   never crosses a terminal), and also exported as bounding boxes in `objects`.
   Only cells no road claimed are painted, so a taxiway running under a
   building outline keeps its class.
6. Service zones become centroid + equivalent radius in `zones`.
7. The sidecar's `max_speed_mps` is the palette maximum in m/s (83.33 for the
   300 km/h runway). A 12 m/s ceiling would drop legal runway movement as a
   GPS jump.
8. Connectivity of the walk mask and of the road mask is counted and printed;
   the walk mask must be one piece.

## Tests

`tests/v2_format.rs` and `tests/v1_compat.rs` prove, against the real geodata:
format/header consistency, unique palette ids, no dangling cell values, layers
agreeing with `palette[].pass`, origin equal to the boundary corner with all
points inside the grid, the four service speed sections surviving, a single
walk component, six objects and ten zones inside the grid, obstacles blocking
routing, byte-identical rebuilds, and the v1 upgrade preserving `road`.
