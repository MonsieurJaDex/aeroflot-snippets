//! A second `startSimulation` on one object, the aircraft's agent kind, and the
//! coordinate the map really reads back.

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;

use super::simulation::{REAL_MAP, connected_cells};
use crate::graph::AgentKind;
use crate::simulation::{agent_of, simulation_key};
use crate::tests::support::{TestContext, expect_status};
use crate::types::enums::VehicleType;

/// Starting the same object again replaces the run that owns its clock. The old
/// driver loop must notice and stop, or two loops advance one clock and the
/// object travels at double speed in visible jumps.
#[tokio::test]
async fn a_second_start_replaces_the_run_that_owns_the_clock() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();
    let (start, end) = connected_cells(&ctx, AgentKind::Vehicle);
    let obj = "vehicle-02";
    let map_id = ctx.active_id();
    let stops = json!([[start.0, start.1], [end.0, end.1]]);

    expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({ "obj": obj, "stops": stops, "tick_ms": 60000 }),
        )
        .await,
        StatusCode::OK,
    );
    let mut redis_conn = ctx.redis();
    let first = crate::simulation::load(&mut redis_conn, &map_id, obj).expect("first run");

    expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({ "obj": obj, "stops": stops, "tick_ms": 60000 }),
        )
        .await,
        StatusCode::OK,
    );
    let second = crate::simulation::load(&mut redis_conn, &map_id, obj).expect("second run");

    assert_ne!(
        first.run, second.run,
        "every start must stamp its own run, so the older driver can tell it lost the clock"
    );
    assert_eq!(second.clock_ms, 0, "a new start begins at the beginning");

    ctx.redis_del(&simulation_key(&map_id, obj));
}

/// An aircraft is not a pedestrian: it is planned on the vehicle side of the
/// graph, where apron and runway speeds live. Only an engineer walks.
#[test]
fn an_aircraft_is_planned_as_vehicles_are() {
    assert_eq!(agent_of("RA-85731"), AgentKind::Vehicle);
    assert_eq!(agent_of("vehicle-03"), AgentKind::Vehicle);
    assert_eq!(agent_of("v-01"), AgentKind::Vehicle);
    assert_eq!(agent_of("AT-0001"), AgentKind::Engineer);
}

/// The map reads its marker from the coordinate key, not from the status
/// endpoint. A simulated object must therefore leave its fraction there.
#[tokio::test]
async fn a_simulated_position_keeps_its_fraction_where_the_map_reads_it() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();
    let (start, end) = connected_cells(&ctx, AgentKind::Vehicle);
    let obj = "vehicle-04";
    let map_id = ctx.active_id();

    expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({ "obj": obj, "stops": [[start.0, start.1], [end.0, end.1]], "tick_ms": 60000 }),
        )
        .await,
        StatusCode::OK,
    );

    let active = ctx.state.active.read().expect("active map").clone();
    let mut redis_conn = ctx.redis();
    let mut pg_conn = ctx.db();
    // Half a second of route time between two cells: the object stands between
    // cells, which is exactly what a whole-cell write would throw away.
    crate::simulation::step(&active, &mut redis_conn, &mut pg_conn, obj, 500).expect("step");

    // The simulator resolves an unregistered vehicle to a fuel truck, and the
    // position key is addressed by the object's own uuid, exactly as ingest does.
    let position_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, obj.as_bytes());
    let key = active.vehicle_position_key(&VehicleType::FuelTruck, position_id);
    let written = ctx
        .redis_get(&key)
        .expect("a simulated point must publish a position the map can read");

    let (x, y) = written.split_once(',').expect("x,y");
    assert!(
        x.contains('.') || y.contains('.'),
        "the coordinate the map reads must carry the fraction, got {written:?}"
    );

    // A reader that wants a cell still gets one: the fraction is rounded, not lost.
    let cell = crate::types::map::Point::from_value(&written).expect("a fraction still parses");
    let smooth = ctx
        .redis_get(&crate::simulation::smooth_key(&map_id, obj))
        .expect("smooth coordinate");
    let (sx, sy) = smooth.split_once(',').expect("x,y");
    let sx: f64 = sx.parse().expect("x");
    let sy: f64 = sy.parse().expect("y");
    assert_eq!(
        (cell.0, cell.1),
        (sx.round() as i64, sy.round() as i64),
        "a cell reader must see the same point rounded, not a stale one"
    );

    ctx.redis_del(&simulation_key(&map_id, obj));
}
