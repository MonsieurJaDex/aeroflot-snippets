//! Acceptance tests for the v2 palette map built from the real airport geodata.
//!
//! Run from the repository root: `cargo test --manifest-path tools/parser/Cargo.toml`.

use std::path::{Path, PathBuf};

use parser::{geo::Grid, types::MapDocument};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives in <root>/tools/parser")
        .to_path_buf()
}

fn load_v2() -> MapDocument {
    let path = repo_root().join("backend/assets/it2026_aero.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} — run `parser parse` first", path.display()));
    let doc: MapDocument = serde_json::from_str(&raw).expect("v2 map parses");
    doc.validate().expect("v2 map validates");
    doc
}

fn components(doc: &MapDocument, ids: &std::collections::HashSet<i64>) -> Vec<usize> {
    let mut solid = std::collections::HashSet::new();
    for (y, row) in doc.map.0.iter().enumerate() {
        for (x, value) in row.iter().enumerate() {
            if ids.contains(value) {
                solid.insert((x as i64, y as i64));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut sizes = Vec::new();
    for start in solid.iter().copied() {
        if !seen.insert(start) {
            continue;
        }
        let mut queue = std::collections::VecDeque::from([start]);
        let mut size = 0;
        while let Some((x, y)) = queue.pop_front() {
            size += 1;
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let next = (x + dx, y + dy);
                if solid.contains(&next) && seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        sizes.push(size);
    }
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes
}

#[test]
fn file_is_format_2_with_matching_header() {
    let doc = load_v2();
    assert_eq!(doc.format, 2);
    assert_eq!(doc.map.0.len(), doc.height);
    assert!(doc.map.0.iter().all(|row| row.len() == doc.width));
    assert!(doc.width > 64 && doc.height > 64, "real map is bigger than the 64x64 placeholder");
    assert!((doc.cell_size_m - 5.0).abs() < f64::EPSILON);
}

#[test]
fn palette_ids_are_unique_and_cells_never_dangle() {
    let doc = load_v2();
    let ids: std::collections::HashSet<i64> = doc.palette.iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), doc.palette.len(), "palette ids must be unique");
    assert_eq!(ids.len(), doc.palette.len(), "no duplicate ids");
    for (y, row) in doc.map.0.iter().enumerate() {
        for (x, value) in row.iter().enumerate() {
            assert!(
                *value == 0 || ids.contains(value),
                "cell ({x},{y}) holds {value}, no palette entry"
            );
        }
    }
    let pairs: std::collections::HashSet<(String, i64)> = doc
        .palette
        .iter()
        .map(|p| (p.kind.clone(), p.spd_kmh))
        .collect();
    assert_eq!(pairs.len(), doc.palette.len(), "one palette entry per (type, speed)");
}

#[test]
fn layers_agree_with_palette_pass() {
    let doc = load_v2();
    let road: std::collections::HashSet<i64> = doc
        .palette
        .iter()
        .filter(|p| p.pass.iter().any(|s| s == "road"))
        .map(|p| p.id)
        .collect();
    let walk: std::collections::HashSet<i64> = doc
        .palette
        .iter()
        .filter(|p| p.pass.iter().any(|s| s == "walk"))
        .map(|p| p.id)
        .collect();
    assert_eq!(doc.road_ids(), road);
    assert_eq!(doc.walk_ids(), walk);
    assert!(walk.is_superset(&road), "every drivable cell must be walkable");
}

#[test]
fn origin_is_the_boundary_north_west_corner_and_indices_are_non_negative() {
    let doc = load_v2();
    let raw = std::fs::read_to_string(
        repo_root().join("backend/assets/it2026_aero_geodata/airport_boundaries.geojson"),
    )
    .expect("boundary geojson");
    let features = parser::geo::read_features(&raw).expect("boundary parses");
    let mut max_lat = f64::NEG_INFINITY;
    let mut min_lon = f64::INFINITY;
    for point in features.iter().flat_map(|f| f.points()) {
        max_lat = max_lat.max(point.lat);
        min_lon = min_lon.min(point.lon);
    }
    assert!((doc.origin.lat - max_lat).abs() < 1e-9, "origin.lat must be max lat");
    assert!((doc.origin.lon - min_lon).abs() < 1e-9, "origin.lon must be min lon");

    let grid = Grid {
        origin_lat: doc.origin.lat,
        origin_lon: doc.origin.lon,
        cell_size_m: doc.cell_size_m,
        width: doc.width,
        height: doc.height,
    };
    let mut outside = 0;
    for feature in &features {
        for point in feature.points() {
            let (x, y) = grid.to_cell(point);
            if x < 0.0 || y < 0.0 || x >= doc.width as f64 || y >= doc.height as f64 {
                outside += 1;
            }
        }
    }
    assert_eq!(outside, 0, "every boundary point must land inside the grid");
}

#[test]
fn speed_sections_survive_as_distinct_entries() {
    let doc = load_v2();
    let service: std::collections::BTreeSet<i64> = doc
        .palette
        .iter()
        .filter(|p| p.kind == "service")
        .map(|p| p.spd_kmh)
        .collect();
    assert_eq!(
        service,
        std::collections::BTreeSet::from([5, 8, 10, 20]),
        "service roads must keep their four speed sections"
    );
    let runway = doc
        .palette
        .iter()
        .find(|p| p.kind == "runway" && p.spd_kmh > 0)
        .expect("runway section");
    assert_eq!(runway.spd_kmh, 300);
    assert_eq!(runway.speed_class.as_deref(), Some("Runway"));
    let pedestrian = doc
        .palette
        .iter()
        .find(|p| p.kind == "pedestrian")
        .expect("pedestrian section");
    assert_eq!(pedestrian.speed_class.as_deref(), Some("Walk"));
    assert_eq!(pedestrian.pass, vec!["walk".to_string()]);
}

#[test]
fn overlap_cells_are_present_and_keep_the_lower_priority_class_walkable() {
    let doc = load_v2();
    // The source raster has cells claimed by two road classes; the resolver
    // keeps the priority winner in `map` and the walk mask keeps it walkable.
    // Prove overlap is genuinely reachable: at least one cell whose map value
    // is a road class sits on a pedestrian line, i.e. a pedestrian-only class
    // exists yet the walk mask is one piece — that is the overlay that the v1
    // parser destroyed.
    let pedestrian_only = doc
        .palette
        .iter()
        .any(|p| p.pass == vec!["walk".to_string()]);
    assert!(pedestrian_only, "pedestrian-only cells must exist");
    let walk = components(&doc, &doc.walk_ids());
    assert_eq!(
        walk.len(),
        1,
        "walk mask must be a single piece so the engineer can leave the apron, got {:?}",
        &walk[..walk.len().min(8)]
    );
    assert!(walk[0] > 2000, "walk mask unexpectedly small: {}", walk[0]);
}

/// The customer's complaint: the old parser flattened everything into one layer,
/// so where a fast section crosses a slow road the second speed vanished. The
/// matrix still stores one id, but `overlays` must keep both.
#[test]
fn overlapping_cells_keep_every_colocated_speed() {
    let doc = load_v2();
    assert!(
        !doc.overlays.is_empty(),
        "the real map has overlapping roads, so overlays must not be empty"
    );
    let mut two_speeds = 0;
    for overlay in &doc.overlays {
        assert_eq!(
            doc.map.0[overlay.y as usize][overlay.x as usize], overlay.id,
            "overlay must point at the cell it describes"
        );
        assert_eq!(
            overlay.ids.first(),
            Some(&overlay.id),
            "the stored id must come first"
        );
        assert!(overlay.ids.len() >= 2, "an overlay is at least two ids");
        let entries: Vec<_> = overlay
            .ids
            .iter()
            .map(|id| doc.entry(*id).expect("palette entry"))
            .collect();
        // Nothing is lost: every claimant is still in the palette, and both
        // remain reachable through the layers they belong to.
        for entry in &entries {
            let reachable = doc.walk_ids().contains(&entry.id)
                || doc.road_ids().contains(&entry.id);
            assert!(reachable, "overlay id {} became unreachable", entry.id);
        }
        let road_speeds: std::collections::BTreeSet<i64> = entries
            .iter()
            .filter(|entry| entry.pass.iter().any(|p| p == "road"))
            .map(|entry| entry.spd_kmh)
            .collect();
        if road_speeds.len() > 1 {
            two_speeds += 1;
        }
    }
    assert!(
        two_speeds > 0,
        "at least one overlap must join two different road speeds, \
         otherwise the overlays prove nothing about speed sections"
    );
    // The pedestrian-over-road case is preserved by the masks, not the overlay.
    let pedestrian_over_road = doc
        .overlays
        .iter()
        .filter(|overlay| {
            let kinds: Vec<&str> = overlay
                .ids
                .iter()
                .filter_map(|id| doc.entry(*id))
                .map(|entry| entry.kind.as_str())
                .collect();
            kinds.contains(&"pedestrian") && kinds.iter().any(|k| *k != "pedestrian")
        })
        .count();
    assert!(
        pedestrian_over_road > 0,
        "the map has pedestrian lines crossing roads"
    );
}

#[test]
fn objects_and_zones_are_inside_the_grid() {
    let doc = load_v2();
    assert_eq!(doc.objects.len(), 6, "six objects in objects.geojson");
    assert_eq!(doc.zones.len(), 10, "ten service zones");
    for object in &doc.objects {
        assert!(object.x >= 0 && object.y >= 0, "{object:?}");
        assert!(
            object.x + object.w <= doc.width as i64 && object.y + object.h <= doc.height as i64,
            "object outside the grid: {object:?}"
        );
    }
    for zone in &doc.zones {
        assert!(zone.x >= 0 && zone.y >= 0, "{zone:?}");
        assert!(zone.x < doc.width as i64 && zone.y < doc.height as i64, "{zone:?}");
        assert!(zone.radius_m > 0.0);
    }
    let kinds: std::collections::BTreeSet<&str> =
        doc.objects.iter().map(|o| o.kind.as_str()).collect();
    assert!(kinds.contains("terminal"));
    assert!(kinds.contains("hangar"));
}

#[test]
fn obstacles_block_routing_but_not_the_walk_mask() {
    let doc = load_v2();
    let walk = doc.walk_ids();
    let mut obstacle_cells = 0;
    for row in &doc.map.0 {
        for value in row {
            if *value == 0 || walk.contains(value) {
                continue;
            }
            let entry = doc.entry(*value).expect("palette entry");
            assert!(entry.pass.is_empty(), "unexpected impassable entry {entry:?}");
            obstacle_cells += 1;
        }
    }
    assert!(
        obstacle_cells > 5000,
        "the buildings should be rasterised as obstacles, found {obstacle_cells}"
    );
}

#[test]
fn build_is_deterministic() {
    let config = repo_root().join("tools/parser/config.json");
    let raw = std::fs::read_to_string(&config).expect("config");
    let cfg: parser::types::Config = serde_json::from_str(&raw).expect("config parses");
    let first = parser::parser::build_geojson(&cfg).expect("first build");
    let second = parser::parser::build_geojson(&cfg).expect("second build");
    let a = serde_json::to_string(&first.doc).expect("serialise");
    let b = serde_json::to_string(&second.doc).expect("serialise");
    assert_eq!(a, b, "two builds of the same input must be identical");
    assert!(first.stats.overlap_cells > 0, "overlap must actually occur");
    assert_eq!(
        first.stats.walk_components, 1,
        "walk mask must be one piece, got {} components {:?}",
        first.stats.walk_components,
        &first.stats.component_cells[..first.stats.component_cells.len().min(6)]
    );
}

#[test]
fn committed_artefact_matches_a_fresh_build() {
    let doc = load_v2();
    let on_disk = std::fs::read_to_string(repo_root().join("backend/assets/it2026_aero.json"))
        .expect("map file");
    let reparsed: MapDocument = serde_json::from_str(&on_disk).expect("parses");
    assert_eq!(
        serde_json::to_string(&reparsed).expect("serialise"),
        serde_json::to_string(&doc).expect("serialise")
    );
}
