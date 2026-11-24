//! The real airport map (`assets/it2026_aero.json`, format 2) driven through the
//! HTTP API: palette-exposed map, routing across the real grid, and the
//! per-section speeds that the v1 format could not express.

use axum::http::StatusCode;
use serde_json::json;

use crate::graph::SpeedClass;
use crate::tests::support::{TestContext, expect_status};
use crate::types::enums::EngineerType;

const REAL_MAP: &str = "assets/it2026_aero.json";

#[tokio::test]
async fn serves_the_palette_map_without_handcoded_gids() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();

    let map = expect_status(ctx.get("/api/map", &dispatcher.token).await, StatusCode::OK);
    assert_eq!(map["format"], 2);
    assert_eq!(map["id"], "it2026_aero");

    let width = map["width"].as_u64().expect("width") as usize;
    let height = map["height"].as_u64().expect("height") as usize;
    assert!(width > 64 && height > 64, "real map must be larger than the placeholder");
    let matrix = map["matrix"].as_array().expect("matrix");
    assert_eq!(matrix.len(), height);
    assert!(matrix.iter().all(|row| row.as_array().unwrap().len() == width));

    let palette = map["palette"].as_array().expect("palette");
    assert!(palette.len() >= 8, "palette must carry the classes, got {}", palette.len());
    let ids: Vec<i64> = palette.iter().map(|entry| entry["id"].as_i64().unwrap()).collect();
    let road: Vec<i64> = map["layers"]["road"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_i64().unwrap())
        .collect();
    let walk: Vec<i64> = map["layers"]["walk"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_i64().unwrap())
        .collect();
    assert!(road.iter().all(|id| ids.contains(id)));
    assert!(road.iter().all(|id| walk.contains(id)), "drivable cells must be walkable");

    // Speed sections are visible to a client without knowing TileD gids.
    let service_speeds: Vec<i64> = palette
        .iter()
        .filter(|entry| entry["type"] == "service")
        .map(|entry| entry["spd_kmh"].as_i64().unwrap())
        .collect();
    assert!(service_speeds.len() >= 4, "service speed sections: {service_speeds:?}");
    assert!(
        palette
            .iter()
            .any(|entry| entry["speed_class"] == "Runway"),
        "the runway section must reach the client"
    );

    // Origin and geometry travel with the map.
    assert!(map["cell_size_m"].as_f64().unwrap() > 0.0);
    assert!(map["origin"]["lat"].as_f64().unwrap() > 55.0);
    assert_eq!(map["objects"].as_array().unwrap().len(), 6);
    assert_eq!(map["zones"].as_array().unwrap().len(), 10);
}

#[tokio::test]
async fn routes_across_the_real_apron_and_rejects_far_outside_points() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();
    let active = ctx.state.active.read().expect("active map").clone();

    // Pick two drivable cells far apart and route between them.
    let mut cells = Vec::new();
    for (y, row) in active.map.0.iter().enumerate() {
        for (x, value) in row.iter().enumerate() {
            if active.document.road_ids().contains(value) {
                cells.push((x as i64, y as i64));
            }
        }
    }
    assert!(cells.len() > 1000, "the real map must have a road network");
    let start = cells[0];
    let end = cells[cells.len() - 1];
    let body = expect_status(
        ctx.post(
            "/api/getRoute",
            &dispatcher.token,
            json!({ "start_point": [start.0, start.1], "end_point": [end.0, end.1] }),
        )
        .await,
        StatusCode::OK,
    );
    let route = body["route"].as_array().unwrap();
    assert!(route.len() > 1);

    expect_status(
        ctx.post(
            "/api/getRoute",
            &dispatcher.token,
            json!({ "start_point": [5, 5], "end_point": [100_000, 100_000] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
}

#[tokio::test]
async fn map_catalog_lists_the_real_map_next_to_the_legacy_ones() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();

    let catalog = expect_status(
        ctx.get("/api/maps", &dispatcher.token).await,
        StatusCode::OK,
    );
    let ids: Vec<&str> = catalog["maps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"it2026_aero"), "catalog: {ids:?}");
    assert!(ids.contains(&"map2"), "the v1 map must stay selectable");
    assert_eq!(catalog["active_id"], "it2026_aero");

    // Switching to the legacy map and back keeps both loadable.
    let active = expect_status(
        ctx.put("/api/maps/active", &dispatcher.token, json!({ "id": "map2" }))
            .await,
        StatusCode::OK,
    );
    assert_eq!(active["width"], 64);
    let back = expect_status(
        ctx.put(
            "/api/maps/active",
            &dispatcher.token,
            json!({ "id": "it2026_aero" }),
        )
        .await,
        StatusCode::OK,
    );
    assert!(back["width"].as_u64().unwrap() > 64);
}

#[tokio::test]
async fn speed_classes_are_per_section_not_flat_apron() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let active = ctx.state.active.read().expect("active map").clone();
    let document = active.document.clone();

    let classes: Vec<SpeedClass> = document
        .palette
        .iter()
        .filter_map(|entry| entry.speed_class.as_deref())
        .map(SpeedClass::from_name)
        .collect();
    assert!(
        classes.contains(&SpeedClass::Runway),
        "palette classes: {classes:?}"
    );
    assert!(
        classes.iter().any(|class| *class != SpeedClass::VehicleApron),
        "not every cell may be apron speed: {classes:?}"
    );

    // A vehicle keeps the airfield cap on a runway section (300 km/h is the
    // aircraft's), while the posted speed stays in the palette.
    let runway = crate::graph::vehicle_speed_mps(
        crate::types::map::Point::new(0, 0),
        SpeedClass::Runway,
    );
    let airfield = crate::graph::vehicle_speed_mps(
        crate::types::map::Point::new(0, 0),
        SpeedClass::VehicleAirfield,
    );
    assert!((runway - airfield).abs() < 1e-9, "a vehicle on the runway is capped");
    // A 300 km/h section still lets a vehicle skip apron cells that are slower.
    let apron = crate::graph::vehicle_speed_mps(
        crate::types::map::Point::new(0, 0),
        SpeedClass::VehicleApron,
    );
    assert!(runway > apron, "runway cap {runway} m/s must beat apron {apron} m/s");
    // And travel time follows the section: a runway edge is cheap for a vehicle.
    assert!(
        runway / apron >= 1.9,
        "sections must actually differ in cost: runway {runway} vs apron {apron} m/s"
    );

    // The GPS plausibility ceiling comes from the palette, not the sidecar's
    // 12 m/s, otherwise a legal runway movement is dropped as a jump.
    let location = &active.location;
    assert!(
        location.max_speed_mps >= runway * 0.99,
        "plausibility ceiling {} m/s must cover the runway {runway} m/s",
        location.max_speed_mps
    );
}

#[tokio::test]
async fn engineer_can_leave_the_apron_on_pedestrian_paths() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let active = ctx.state.active.read().expect("active map").clone();

    let walk_only = active
        .document
        .palette
        .iter()
        .find(|entry| entry.allows_walk() && !entry.allows_road())
        .expect("the map must carry pedestrian-only cells");
    let walk_only_cell = active
        .map
        .0
        .iter()
        .enumerate()
        .find_map(|(y, row)| {
            row.iter()
                .position(|value| *value == walk_only.id)
                .map(|x| (x as i64, y as i64))
        })
        .expect("a pedestrian-only cell exists in the matrix");
    let road_cell = active
        .map
        .0
        .iter()
        .enumerate()
        .find_map(|(y, row)| {
            row.iter()
                .position(|value| active.document.road_ids().contains(value))
                .map(|x| (x as i64, y as i64))
        })
        .expect("a drivable cell exists");

    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    expect_status(
        ctx.post(
            "/api/getRoute",
            &engineer.token,
            json!({
                "start_point": [road_cell.0, road_cell.1],
                "end_point": [walk_only_cell.0, walk_only_cell.1],
            }),
        )
        .await,
        StatusCode::OK,
    );
}
