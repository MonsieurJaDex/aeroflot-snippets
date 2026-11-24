use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fs,
    io::{self, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    geo::{GeoFeature, Grid, LonLat, point_in_ring, read_features, ring_area_m2},
    types::{
        AppConfig, Config, MapDocument, MapLayers, MapMatrix, MapObject, MapZone, Origin,
        PaletteEntry, Sidecar, SidecarObject, SidecarZone, TiledLayer, TmjFile,
    },
};

/// Root-level shape of a v1 map file.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonMatrix {
    pub map: MapMatrix,
    pub road: Vec<i64>,
}

pub fn init_config_file(app_config: &AppConfig) -> io::Result<()> {
    let file = fs::File::create(&app_config.config_filepath)?;

    let initial_cfg = Config::new("".to_string(), vec![]);
    serde_json::to_writer_pretty(file, &initial_cfg)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// TMJ -> v1 (unchanged behaviour of the original parser)
// ---------------------------------------------------------------------------

pub fn parse_tmj(config: &Config, app_config: &AppConfig) -> io::Result<()> {
    let map_path = crate::types::resolve(&config.map_path);
    let raw = fs::read_to_string(&map_path)?;
    let raw_map: TmjFile = serde_json::from_str(&raw)?;
    let layers: Vec<TiledLayer> = raw_map.layers;

    if layers.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "provided layers were empty",
        ));
    }

    let tile_layers: Vec<TiledLayer> = layers
        .into_iter()
        .filter(TiledLayer::is_tile_layer)
        .filter(|layer| layer.width > 0 && !layer.data.is_empty())
        .collect();
    let Some(first) = tile_layers.first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no tile layer with data in the map",
        ));
    };
    let mut matrix = MapMatrix::new(first.width as usize, first.height.max(0) as usize, 0);
    for layer in tile_layers {
        if layer.width == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "layer width is zero",
            ));
        }
        for (matrix_row, chunk_row) in matrix.0.iter_mut().zip(layer.data.chunks(layer.width as usize)) {
            for (matrix_cell, chunk_cell) in matrix_row.iter_mut().zip(chunk_row) {
                if chunk_cell.eq(&0) {
                    continue;
                }
                *matrix_cell = *chunk_cell
            }
        }
    }

    let json_matrix = JsonMatrix {
        map: matrix,
        road: config.roads.clone(),
    };
    let file = fs::File::create(&app_config.parsed_map_filepath)?;
    serde_json::to_writer_pretty(file, &json_matrix)?;
    println!(
        "v1 (tmj): {}x{}, road gids {:?}",
        json_matrix.map.width(),
        json_matrix.map.height(),
        json_matrix.road
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// v1 -> v2 upgrade (keeps the sidecar untouched)
// ---------------------------------------------------------------------------

pub fn upgrade_v1(config: &Config) -> io::Result<()> {
    let map_path = crate::types::resolve(&config.map_path);
    let raw = fs::read_to_string(&map_path)?;
    let v1: JsonMatrix = serde_json::from_str(&raw)?;
    let mut matrix = v1.map.clone();
    if matrix.width() == 0 || matrix.height() == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty v1 map"));
    }

    let mut observed: BTreeSet<i64> = matrix.0.iter().flatten().copied().filter(|v| *v != 0).collect();
    let mut palette = Vec::new();
    let mut road: Vec<i64> = v1.road.clone();
    road.sort_unstable();
    road.dedup();
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
    let doc = MapDocument {
        format: 2,
        name: config.name.clone(),
        cell_size_m: config.cell_size_m,
        origin: Origin {
            lat: 55.831,
            lon: 37.636,
        },
        width: matrix.width(),
        height: matrix.height(),
        objects: Vec::new(),
        zones: Vec::new(),
        closed_cells: Vec::new(),
        map: std::mem::take(&mut matrix),
        palette,
        layers,
    };
    write_document(config, &doc)
}

fn write_document(config: &Config, doc: &MapDocument) -> io::Result<()> {
    doc.validate()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let out = config.out_path();
    let mut file = fs::File::create(&out)?;
    serde_json::to_writer_pretty(&mut file, doc)?;
    file.flush()?;
    println!(
        "v2 -> {}: {}x{} cells @ {} m, palette {}, road ids {}, walk ids {}",
        out.display(),
        doc.width,
        doc.height,
        doc.cell_size_m,
        doc.palette.len(),
        doc.layers.road.len(),
        doc.layers.walk.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// GeoJSON -> v2
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct RoadClass {
    kind: &'static str,
    spd_kmh: i64,
}

impl RoadClass {
    fn speed_class(self) -> Option<&'static str> {
        Some(match (self.kind, self.spd_kmh) {
            ("pedestrian", _) => "Walk",
            ("runway", _) => "Runway",
            (_, s) if s <= 5 => "VehicleStand",
            (_, s) if s <= 8 => "VehicleTerminal",
            (_, s) if s <= 10 => "VehicleApron",
            (_, _) => "VehicleAirfield",
        })
    }

    fn pass(self) -> Vec<String> {
        match self.kind {
            "pedestrian" => vec!["walk".to_string()],
            _ => vec!["road".to_string(), "walk".to_string()],
        }
    }

    fn allows_road(self) -> bool {
        self.kind != "pedestrian"
    }
}

/// Building / obstacle entry. `object_index` keeps holes and separate polygons
/// apart when rasterising: cell is solid if an odd number of rings contain it.
#[derive(Debug, Clone)]
struct Obstacle {
    entry: PaletteEntry,
    rings: Vec<Vec<LonLat>>,
}

fn read_geojson_file(path: &Path) -> Result<Vec<GeoFeature>, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    read_features(&raw).map_err(|e| format!("{}: {e}", path.display()))
}

/// Normalises a road feature's `type` property.
fn road_kind(feature: &GeoFeature) -> &'static str {
    match feature.prop_str("type").unwrap_or_default().as_str() {
        "service" => "service",
        "taxi" => "taxi",
        "pedestrian" => "pedestrian",
        "runway" => "runway",
        _ => "service",
    }
}

fn road_spd(feature: &GeoFeature) -> i64 {
    feature
        .prop_f64("spd_kmh")
        .map(|v| v.round() as i64)
        .filter(|v| *v > 0)
        .unwrap_or_else(|| match road_kind(feature) {
            "pedestrian" => 5,
            "runway" => 300,
            _ => 10,
        })
}

fn object_kind_mapping(name: &str) -> &'static str {
    if name.starts_with("Терминал") {
        "terminal"
    } else if name.starts_with("Ангар") {
        "hangar"
    } else if name.starts_with("ВПП") || name.starts_with("Взлёт") {
        "runway"
    } else if name.to_lowercase().contains("стоянка") {
        "parking"
    } else if name.starts_with("Склад") {
        "warehouse"
    } else {
        "other"
    }
}

pub struct BuildResult {
    pub doc: MapDocument,
    pub stats: Stats,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub road_cells: usize,
    pub per_class: BTreeMap<String, usize>,
    pub overlap_cells: usize,
    pub walk_components: usize,
    pub road_components: usize,
    pub stitched_cells: usize,
    pub component_cells: Vec<usize>,
}

/// Samples a segment in the cell plane, half a cell apart, and records a
/// candidate class for every cell the line passes through.
fn raster_line(
    grid: &Grid,
    from: LonLat,
    to: LonLat,
    class: RoadClass,
    cells: &mut HashMap<(i64, i64), Vec<RoadClass>>,
) {
    let (x0, y0) = grid.to_cell(from);
    let (x1, y1) = grid.to_cell(to);
    let dx = x1 - x0;
    let dy = y1 - y0;
    let steps = dx.abs().max(dy.abs()).ceil().max(1.0) * 2.0;
    let mut previous: Option<(i64, i64)> = None;
    let mut mark = |cell: (i64, i64), previous: &mut Option<(i64, i64)>| {
        if !grid.in_bounds(cell.0, cell.1) || *previous == Some(cell) {
            return;
        }
        cells.entry(cell).or_default().push(class);
        *previous = Some(cell);
    };
    for i in 0..=(steps as i64) {
        let t = i as f64 / steps;
        let x = x0 + dx * t;
        let y = y0 + dy * t;
        let cell = (x.floor() as i64, y.floor() as i64);
        // Supercover: a diagonal step would break 4-neighbour connectivity, so
        // paint the intermediate cell as well. Without this the raster falls
        // apart into hundreds of one-cell fragments.
        if let Some(prev) = previous {
            let step_x = cell.0 - prev.0;
            let step_y = cell.1 - prev.1;
            if step_x != 0 && step_y != 0 {
                let corner_a = (prev.0 + step_x, prev.1);
                let corner_b = (prev.0, prev.1 + step_y);
                // The corner whose centre the segment actually passes nearer.
                let (mx, my) = (x - cell.0 as f64, y - cell.1 as f64);
                let pick = if mx > my { corner_b } else { corner_a };
                mark(pick, &mut previous);
            }
        }
        mark(cell, &mut previous);
    }
}

fn class_rank(priority: &[String], class: RoadClass) -> usize {
    priority
        .iter()
        .position(|p| p == class.kind)
        .unwrap_or(priority.len())
}

fn component_count(grid: &Grid, solid: &HashSet<(i64, i64)>, four: bool) -> (usize, Vec<usize>) {
    let mut seen: HashSet<(i64, i64)> = HashSet::new();
    let mut sizes = Vec::new();
    let steps: &[(i64, i64)] = if four {
        &[(1, 0), (-1, 0), (0, 1), (0, -1)]
    } else {
        &[(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)]
    };
    for start in solid.iter().copied() {
        if !seen.insert(start) {
            continue;
        }
        let mut queue = VecDeque::from([start]);
        let mut size = 0usize;
        while let Some((x, y)) = queue.pop_front() {
            size += 1;
            for (dx, dy) in steps {
                let next = (x + dx, y + dy);
                if grid.in_bounds(next.0, next.1) && solid.contains(&next) && seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        sizes.push(size);
    }
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    (sizes.len(), sizes)
}

pub fn parse_geojson(config: &Config) -> io::Result<()> {
    let summary = build_geojson(config)?;
    write_document(config, &summary.doc)?;
    print_stats(&summary);
    write_sidecar(config, &summary)?;
    Ok(())
}

/// Builds the v2 document without writing it, together with the counters that
/// explain the result. Everything `parse_geojson` does except the filesystem.
pub fn build_geojson(config: &Config) -> io::Result<BuildResult> {
    let dir = config.input_path();
    let dir = dir.as_path();
    let boundary = read_geojson_file(&dir.join(&config.boundary_file))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let roads = read_geojson_file(&dir.join(&config.roads_file))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let objects = read_geojson_file(&dir.join(&config.objects_file))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let zones = read_geojson_file(&dir.join(&config.zones_file))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // 1. Grid from the airport boundary, north-west corner anchor.
    let grid = Grid::covering(&boundary, config.cell_size_m, 0).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "airport boundary has no coordinates")
    })?;

    // 2. Obstacles. One palette entry per obstacle kind, one obstacle per
    // feature (rings are rasterised together, so holes work).
    let mut obstacle_kinds: BTreeSet<&'static str> = BTreeSet::new();
    let mut obstacles: Vec<Obstacle> = Vec::new();
    for (index, feature) in objects.iter().enumerate() {
        let name = feature
            .prop_str("name")
            .unwrap_or_else(|| format!("object-{index}"));
        let kind = object_kind_mapping(&name);
        obstacle_kinds.insert(kind);
        obstacles.push(Obstacle {
            entry: PaletteEntry {
                id: 0,
                kind: kind.into(),
                spd_kmh: 0,
                speed_class: None,
                pass: Vec::new(),
            },
            rings: feature.rings.clone(),
        });
    }

    // 3. Road candidates per cell.
    let mut cells: HashMap<(i64, i64), Vec<RoadClass>> = HashMap::new();
    for feature in &roads {
        let class = RoadClass {
            kind: road_kind(feature),
            spd_kmh: road_spd(feature),
        };
        for ring in &feature.rings {
            for pair in ring.windows(2) {
                raster_line(&grid, pair[0], pair[1], class, &mut cells);
            }
        }
    }

    // 4. Palette: one entry per (kind, spd), ordered by priority then speed.
    let mut classes: Vec<RoadClass> = cells.values().flatten().copied().collect::<BTreeSet<_>>().into_iter().collect();
    classes.sort_by_key(|c| (class_rank(&config.priority, *c), c.spd_kmh, c.kind));
    let mut palette: Vec<PaletteEntry> = Vec::new();
    let mut id_of: BTreeMap<RoadClass, i64> = BTreeMap::new();
    for (index, class) in classes.iter().enumerate() {
        let id = index as i64 + 1;
        id_of.insert(*class, id);
        palette.push(PaletteEntry {
            id,
            kind: class.kind.to_string(),
            spd_kmh: class.spd_kmh,
            speed_class: class.speed_class().map(str::to_string),
            pass: class.pass(),
        });
    }
    // Register one palette entry per obstacle kind, after the road palette.
    let mut matrix = MapMatrix::new(grid.width, grid.height, 0);
    let mut obstacle_ids: BTreeMap<&'static str, i64> = BTreeMap::new();
    for kind in &obstacle_kinds {
        let id = palette.len() as i64 + 1;
        obstacle_ids.insert(kind, id);
        palette.push(PaletteEntry {
            id,
            kind: (*kind).into(),
            spd_kmh: 0,
            speed_class: None,
            pass: Vec::new(),
        });
    }
    let mut stats = Stats::default();

    // 5. Resolve every road cell by priority, ties by higher speed.
    let mut road_cells: Vec<(i64, i64)> = Vec::new();
    for ((x, y), candidates) in &cells {
        let best = candidates
            .iter()
            .copied()
            .max_by_key(|c| (class_rank(&config.priority, *c), c.spd_kmh))
            .expect("non-empty candidates");
        let id = id_of[&best];
        matrix.0[*y as usize][*x as usize] = id;
        road_cells.push((*x, *y));
        *stats
            .per_class
            .entry(format!("{}_{}", best.kind, best.spd_kmh))
            .or_default() += 1;
        if candidates.iter().copied().collect::<BTreeSet<_>>().len() > 1 {
            stats.overlap_cells += 1;
        }
    }
    stats.road_cells = road_cells.len();

    // 6. Obstacles: only cells that no road claimed.
    for obstacle in &mut obstacles {
        obstacle.entry.id = obstacle_ids[obstacle.entry.kind.as_str()];
        let mut painted = 0usize;
        for ring in &obstacle.rings {
            if let Some((x0, y0, w, h)) = crate::geo::ring_bbox(&grid, ring) {
                for y in y0..y0 + h {
                    for x in x0..x0 + w {
                        if !grid.in_bounds(x, y) {
                            continue;
                        }
                        let inside = obstacle
                            .rings
                            .iter()
                            .filter(|r| point_in_ring(&grid, r, x as f64 + 0.5, y as f64 + 0.5))
                            .count()
                            % 2
                            == 1;
                        if inside && matrix.0[y as usize][x as usize] == 0 {
                            matrix.0[y as usize][x as usize] = obstacle.entry.id;
                            painted += 1;
                        }
                    }
                }
            }
        }
        *stats
            .per_class
            .entry(format!("obstacle_{}", obstacle.entry.kind))
            .or_default() += painted;
    }

    // 7. Walk mask, stitching, connectivity.
    let walk_ids: HashSet<i64> = palette
        .iter()
        .filter(|e| e.pass.iter().any(|p| p == "walk"))
        .map(|e| e.id)
        .collect();
    let road_ids: HashSet<i64> = palette
        .iter()
        .filter(|e| e.pass.iter().any(|p| p == "road"))
        .map(|e| e.id)
        .collect();

    let solid_of = |matrix: &MapMatrix, ids: &HashSet<i64>| -> HashSet<(i64, i64)> {
        let mut solid = HashSet::new();
        for (y, row) in matrix.0.iter().enumerate() {
            for (x, value) in row.iter().enumerate() {
                if ids.contains(value) {
                    solid.insert((x as i64, y as i64));
                }
            }
        }
        solid
    };

    let walk_solid = solid_of(&matrix, &walk_ids);
    let (walk_components, sizes) = component_count(&grid, &walk_solid, true);
    stats.walk_components = walk_components;
    stats.component_cells = sizes;
    stats.road_components = component_count(&grid, &solid_of(&matrix, &road_ids), true).0;

    // Bridge every road-less walk component to the nearest road cell.
    let pedestrian_walkable: HashSet<i64> = walk_ids
        .iter()
        .copied()
        .filter(|id| !road_ids.contains(id))
        .collect();
    if !pedestrian_walkable.is_empty() && config.stitch_radius_cells >= 0 {
        let bridge_id = id_of
            .iter()
            .filter(|(class, _)| !class.allows_road())
            .map(|(_, id)| *id)
            .min();
        if let Some(bridge_id) = bridge_id {
            let road_solid = solid_of(&matrix, &road_ids);
            let mut seen: HashSet<(i64, i64)> = HashSet::new();
            let ped_cells = solid_of(&matrix, &pedestrian_walkable);
            for start in &ped_cells {
                if !seen.insert(*start) {
                    continue;
                }
                let mut queue = VecDeque::from([*start]);
                let mut component = Vec::new();
                while let Some((x, y)) = queue.pop_front() {
                    component.push((x, y));
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let next = (x + dx, y + dy);
                        if grid.in_bounds(next.0, next.1)
                            && !seen.contains(&next)
                            && ped_cells.contains(&next)
                        {
                            seen.insert(next);
                            queue.push_back(next);
                        }
                    }
                }
                if component.iter().any(|c| touches(c, &road_solid)) {
                    continue;
                }
                let painted = bridge_to_road(
                    &mut matrix,
                    &grid,
                    &component,
                    &road_solid,
                    bridge_id,
                    config.stitch_radius_cells,
                );
                stats.stitched_cells += painted;
            }
        }
    }

    // 8. Objects and zones in cells.
    let mut map_objects = Vec::new();
    for (index, feature) in objects.iter().enumerate() {
        let name = feature
            .prop_str("name")
            .unwrap_or_else(|| format!("object-{index}"));
        let id = feature
            .prop_str("id")
            .unwrap_or_else(|| index.to_string());
        let rings = &feature.rings;
        let mut min_x = i64::MAX;
        let mut min_y = i64::MAX;
        let mut max_x = i64::MIN;
        let mut max_y = i64::MIN;
        for ring in rings {
            if let Some((x, y, w, h)) = crate::geo::ring_bbox(&grid, ring) {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + w);
                max_y = max_y.max(y + h);
            }
        }
        if min_x == i64::MAX {
            continue;
        }
        map_objects.push(MapObject {
            id,
            kind: object_kind_mapping(&name).to_string(),
            name,
            x: min_x,
            y: min_y,
            w: (max_x - min_x).max(1),
            h: (max_y - min_y).max(1),
        });
    }

    let mut map_zones = Vec::new();
    for (index, feature) in zones.iter().enumerate() {
        let Some(outer) = feature.outer() else { continue };
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
        for point in outer {
            let (x, y) = grid.to_cell(*point);
            sx += x;
            sy += y;
            n += 1.0;
        }
        if n == 0.0 {
            continue;
        }
        let area = ring_area_m2(&grid, outer);
        map_zones.push(MapZone {
            id: feature
                .prop_str("id")
                .unwrap_or_else(|| format!("sz-{index}")),
            label: feature
                .prop_str("name")
                .unwrap_or_else(|| format!("Зона {}", index + 1)),
            x: (sx / n).round() as i64,
            y: (sy / n).round() as i64,
            radius_m: (area / std::f64::consts::PI).sqrt(),
        });
    }

    let layers = MapLayers {
        road: {
            let mut ids: Vec<i64> = road_ids.iter().copied().collect();
            ids.sort_unstable();
            ids
        },
        walk: {
            let mut ids: Vec<i64> = walk_ids.iter().copied().collect();
            ids.sort_unstable();
            ids
        },
    };

    let doc = MapDocument {
        format: 2,
        name: config.name.clone(),
        cell_size_m: grid.cell_size_m,
        origin: Origin {
            lat: grid.origin_lat,
            lon: grid.origin_lon,
        },
        width: grid.width,
        height: grid.height,
        map: matrix,
        palette,
        layers,
        objects: map_objects,
        zones: map_zones,
        closed_cells: Vec::new(),
    };

    Ok(BuildResult { doc, stats })
}

fn touches(cell: &(i64, i64), road: &HashSet<(i64, i64)>) -> bool {
    [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .iter()
        .any(|(dx, dy)| road.contains(&(cell.0 + dx, cell.1 + dy)))
}

/// Paints a shortest 4-neighbour path from `component` to the nearest road cell.
fn bridge_to_road(
    matrix: &mut MapMatrix,
    grid: &Grid,
    component: &[(i64, i64)],
    road: &HashSet<(i64, i64)>,
    bridge_id: i64,
    radius: i64,
) -> usize {
    let limit = radius.max(1) * 8;
    let start_set: HashSet<(i64, i64)> = component.iter().copied().collect();
    let mut previous: HashMap<(i64, i64), (i64, i64)> = HashMap::new();
    let mut queue: VecDeque<((i64, i64), i64)> =
        component.iter().map(|c| (*c, 0i64)).collect();
    let mut visited: HashSet<(i64, i64)> = start_set.clone();
    let mut target = None;
    while let Some(((x, y), depth)) = queue.pop_front() {
        if depth > limit {
            break;
        }
        if road.contains(&(x, y)) {
            target = Some((x, y));
            break;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = (x + dx, y + dy);
            if !grid.in_bounds(next.0, next.1) || !visited.insert(next) {
                continue;
            }
            previous.insert(next, (x, y));
            queue.push_back((next, depth + 1));
        }
    }
    let Some(mut cursor) = target else {
        return 0;
    };
    let mut painted = 0;
    while let Some(prev) = previous.get(&cursor).copied() {
        if !start_set.contains(&cursor) && matrix.0[cursor.1 as usize][cursor.0 as usize] == 0 {
            matrix.0[cursor.1 as usize][cursor.0 as usize] = bridge_id;
            painted += 1;
        }
        cursor = prev;
        if start_set.contains(&cursor) {
            break;
        }
    }
    painted
}

fn print_stats(result: &BuildResult) {
    let s = &result.stats;
    println!("--- stats ---");
    println!("cells: {}x{} @ {} m", result.doc.width, result.doc.height, result.doc.cell_size_m);
    println!("origin NW: lat {:.6}, lon {:.6}", result.doc.origin.lat, result.doc.origin.lon);
    println!("road cells: {}", s.road_cells);
    println!("overlap cells (>=2 classes claimed): {}", s.overlap_cells);
    let mut classes: Vec<_> = s.per_class.iter().collect();
    classes.sort_by(|a, b| b.1.cmp(a.1));
    for (name, count) in classes {
        println!("  {name}: {count}");
    }
    println!("walk components: {}", s.walk_components);
    println!("road components: {}", s.road_components);
    println!("largest walk components: {:?}", &s.component_cells.iter().take(8).collect::<Vec<_>>());
    println!("stitched cells: {}", s.stitched_cells);
}

fn write_sidecar(config: &Config, result: &BuildResult) -> io::Result<()> {
    let mut sidecar = config
        .runtime_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str::<Sidecar>(&raw).ok())
        .unwrap_or_default();
    let doc = &result.doc;
    sidecar.cell_size_m = doc.cell_size_m;
    sidecar.origin_lat = doc.origin.lat;
    sidecar.origin_lon = doc.origin.lon;
    sidecar.sla_secs = config.sla_secs;
    sidecar.alert_before_secs = config.alert_before_secs;
    sidecar.boarding_delay_secs = config.boarding_delay_secs;
    sidecar.tool_delay_secs = config.tool_delay_secs;
    sidecar.max_accuracy_m = config.max_accuracy_m;
    sidecar.stale_after_secs = config.stale_after_secs;
    sidecar.max_speed_mps = doc.max_spd_kmh() / 3.6;
    sidecar.zones = doc
        .zones
        .iter()
        .map(|z| SidecarZone {
            id: z.id.clone(),
            x: z.x,
            y: z.y,
            radius_m: z.radius_m,
            label: z.label.clone(),
        })
        .collect();
    sidecar.objects = doc
        .objects
        .iter()
        .map(|o| SidecarObject {
            id: o.id.clone(),
            kind: o.kind.clone(),
            x: o.x,
            y: o.y,
        })
        .collect();
    sidecar.closed_cells = doc.closed_cells.clone();

    let path = config.location_path();
    let mut file = fs::File::create(&path)?;
    serde_json::to_writer_pretty(&mut file, &sidecar)?;
    file.flush()?;
    println!(
        "sidecar -> {}: max_speed_mps {:.2}, zones {}, objects {}",
        path.display(),
        sidecar.max_speed_mps,
        sidecar.zones.len(),
        sidecar.objects.len()
    );
    Ok(())
}
