//! Simulation of movement: the route's per-section speeds, the smooth
//! interpolation between cells, the journal of a movement starting, and the
//! pause the organiser checklist asks for.

use axum::http::StatusCode;
use diesel::prelude::*;
use serde_json::json;

use crate::graph::{AgentKind, PlannedRoute, RouteSegment, SpeedClass};
use crate::simulation::{sample_at, simulation_key, smooth_key};
use crate::tests::support::{TestContext, expect_status};
use crate::types::enums::EngineerType;
use crate::types::map::Point;

pub(super) const REAL_MAP: &str = "assets/it2026_aero.json";

/// Two drivable cells joined by a real route on `map`.
pub(super) fn connected_cells(ctx: &TestContext, agent: AgentKind) -> (Point, Point) {
    let active = ctx.state.active.read().expect("active map").clone();
    let road = active.document.road_ids();
    let mut cells = Vec::new();
    for (y, row) in active.map.0.iter().enumerate() {
        for (x, value) in row.iter().enumerate() {
            if road.contains(value) {
                cells.push(Point::new(x as i64, y as i64));
            }
        }
    }
    assert!(cells.len() > 100, "the real map must have a road network");
    let start = cells[0];
    // A route long enough to be sampled part-way, not a single cell hop.
    for candidate in cells.iter().rev() {
        if let Ok(route) = active
            .location
            .graph
            .route_stops(agent, &[start, *candidate])
        {
            if route.seconds > 20.0 {
                return (start, *candidate);
            }
        }
    }
    panic!("no two road cells of the real map are joined by a route");
}

/// The route the graph plans carries the speed of every section it walks, so a
/// caller can time each piece instead of only knowing the total.
#[tokio::test]
async fn planned_route_carries_a_timed_segment_per_walked_edge() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let active = ctx.state.active.read().expect("active map").clone();
    let (start, end) = connected_cells(&ctx, AgentKind::Vehicle);
    let route = active
        .location
        .graph
        .route_stops(AgentKind::Vehicle, &[start, end])
        .expect("the two cells must route");

    assert!(route.points.len() > 2, "the route must cross cells");
    assert!(
        route.segments.len() >= route.points.len() - 1,
        "every hop between cells must be a segment: {} segments for {} points",
        route.segments.len(),
        route.points.len()
    );

    // Times add up to the route's own total, and cumulative time is monotone so a
    // caller can look a segment up by elapsed time.
    let sum: f64 = route.segments.iter().map(|segment| segment.seconds).sum();
    assert!(
        (sum - route.seconds).abs() < 1e-6,
        "segments must sum to the route time: {sum} vs {}",
        route.seconds
    );
    let mut previous = 0.0;
    for segment in &route.segments {
        assert!(segment.cum_seconds > previous);
        assert!(segment.length_m > 0.0 && segment.speed_mps > 0.0);
        assert!((segment.length_m / segment.speed_mps - segment.seconds).abs() < 1e-9);
        previous = segment.cum_seconds;
    }
    assert!((previous - route.seconds).abs() < 1e-6);

    // The per-section speeds are the graph's own, not one constant for the route:
    // a vehicle route over the apron and the airfield must show more than one.
    let mut speeds: Vec<String> = route
        .segments
        .iter()
        .map(|segment| format!("{:?}@{:.3}", segment.class, segment.speed_mps))
        .collect();
    speeds.sort();
    speeds.dedup();
    assert!(
        !speeds.is_empty(),
        "the route must expose the class and the speed of its sections"
    );
}

/// A route with two sections of different speeds is travelled proportionally to
/// those speeds: the same distance on the slow section takes longer, and the
/// position between cells is fractional instead of a jump.
#[test]
fn samples_between_cells_follow_each_segment_speed() {
    // 10 m at 1 m/s (10 s), then 10 m at 10 m/s (1 s).
    let route = PlannedRoute {
        points: vec![Point::new(0, 0), Point::new(10, 0), Point::new(20, 0)],
        seconds: 11.0,
        segments: vec![
            RouteSegment {
                from: Point::new(0, 0),
                to: Point::new(10, 0),
                length_m: 10.0,
                speed_mps: 1.0,
                seconds: 10.0,
                cum_seconds: 10.0,
                class: SpeedClass::Walk,
            },
            RouteSegment {
                from: Point::new(10, 0),
                to: Point::new(20, 0),
                length_m: 10.0,
                speed_mps: 10.0,
                seconds: 1.0,
                cum_seconds: 11.0,
                class: SpeedClass::VehicleAirfield,
            },
        ],
    };

    let half_of_the_slow_part = sample_at(&route, 5.0);
    assert!((half_of_the_slow_part.x - 5.0).abs() < 1e-9);
    assert!(!half_of_the_slow_part.done);
    assert!((half_of_the_slow_part.remaining_seconds - 6.0).abs() < 1e-9);

    // At 10.5 s the object is half-way through the fast section, not half-way
    // along the route: 10 + 0.5 * 10.
    let into_the_fast_part = sample_at(&route, 10.5);
    assert!((into_the_fast_part.x - 15.0).abs() < 1e-9);

    let arrived = sample_at(&route, 11.0);
    assert!(arrived.done);
    assert!((arrived.x - 20.0).abs() < 1e-9);
    assert!(arrived.remaining_seconds.abs() < 1e-9);

    // Beyond the end stays at the end, before the start stays at the start.
    assert!((sample_at(&route, 99.0).x - 20.0).abs() < 1e-9);
    assert!((sample_at(&route, -3.0).x - 0.0).abs() < 1e-9);
}

/// A route almost never lasts a whole number of milliseconds, and the clock is
/// stored in whole milliseconds. Reaching the rounded end must count as arrival,
/// otherwise the object stands on its last cell forever, `active` stays true and
/// the journal never records the finish.
#[test]
fn a_route_that_does_not_end_on_a_whole_millisecond_still_finishes() {
    let route = PlannedRoute {
        points: vec![Point::new(0, 0), Point::new(10, 0)],
        seconds: 448.20000000000086,
        segments: vec![RouteSegment {
            from: Point::new(0, 0),
            to: Point::new(10, 0),
            length_m: 448.2,
            speed_mps: 1.0,
            seconds: 448.20000000000086,
            cum_seconds: 448.20000000000086,
            class: SpeedClass::VehicleAirfield,
        }],
    };

    // The clock the driver stops at: the route's own length, rounded to the
    // millisecond it is stored as.
    let at_the_cap = (route.seconds * 1000.0).round() / 1000.0;
    let arrived = sample_at(&route, at_the_cap);
    assert!(
        arrived.done,
        "the rounded end is the end, not a hair short of it"
    );
    assert!((arrived.x - 10.0).abs() < 1e-9);
    assert!(arrived.remaining_seconds.abs() < 1e-9);

    // One millisecond earlier is still on the way.
    assert!(!sample_at(&route, at_the_cap - 0.001).done);
}

/// The organiser requirement end to end: a movement starts, its positions land
/// where the app already reads them, they are fractional between cells, and the
/// start of the movement is in the journal.
#[tokio::test]
async fn starting_a_movement_publishes_smooth_positions_and_journals_the_start() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();
    let (start, end) = connected_cells(&ctx, AgentKind::Engineer);
    let obj = "AT-0001";

    let body = expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({
                "obj": obj,
                "stops": [[start.0, start.1], [end.0, end.1]],
                "tick_ms": 60000,
                "speed_scale": 1.0,
            }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(body["obj"], obj);
    assert!(body["points"].as_array().unwrap().len() > 2);
    assert!(body["seconds"].as_f64().unwrap() > 20.0);
    assert!(body["started_at"].is_string());

    // The start is in the journal, named for the object, before any tick.
    let mut conn = ctx.db();
    let start_params: Vec<String> = crate::database::schema::journal::table
        .filter(crate::database::schema::journal::kind.eq("simulation_started"))
        .filter(crate::database::schema::journal::actor.eq(Some(obj.to_owned())))
        .select(crate::database::schema::journal::params)
        .load(&mut conn)
        .expect("journal query");
    assert_eq!(
        start_params.len(),
        1,
        "the movement start must be journalled once"
    );
    let params: serde_json::Value = serde_json::from_str(&start_params[0]).expect("params json");
    assert_eq!(params["obj"], obj);
    assert_eq!(params["points"].as_u64().unwrap() > 2, true);

    // The object is on the map at its start cell before it has travelled: the
    // driver publishes the plan's first point as soon as it runs.
    let status = expect_status(
        ctx.get(
            &format!("/api/geo/simulationStatus?obj={obj}"),
            &dispatcher.token,
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(status["active"], true);
    let lat = status["lat"].as_f64().expect("lat");
    let lon = status["lon"].as_f64().expect("lon");
    assert!(lat > 55.0 && lon > 37.0, "position in WGS-84: {status}");
    let total = body["seconds"].as_f64().unwrap();
    assert_eq!(status["total_seconds"].as_f64().unwrap(), total);

    // Advancing by a known stretch of wall time moves the object along the route
    // by exactly that much, and the position it publishes is between cells.
    let map_id = ctx.active_id();
    let active = ctx.state.active.read().expect("active map").clone();
    let mut redis_conn = ctx.redis();
    let mut pg_conn = ctx.db();
    let stepped = crate::simulation::step(&active, &mut redis_conn, &mut pg_conn, obj, 30_000)
        .expect("the simulation must advance");
    let left = stepped.sample.remaining_seconds;
    assert!(
        (total - left - 30.0).abs() < 1e-6,
        "30 s of travel must consume 30 s of route: {total} - {left}"
    );
    let moved = expect_status(
        ctx.get(
            &format!("/api/geo/simulationStatus?obj={obj}"),
            &dispatcher.token,
        )
        .await,
        StatusCode::OK,
    );
    assert!((moved["remaining_seconds"].as_f64().unwrap() - left).abs() < 1e-6);
    let lat = moved["lat"].as_f64().expect("lat");
    let _lon = moved["lon"].as_f64().expect("lon");

    // The smooth coordinate exists and carries a fraction, which the integer cell
    // keys cannot.
    let smooth = ctx
        .redis_get(&smooth_key(&map_id, obj))
        .expect("the smooth coordinate must be stored");
    let (x, y) = smooth.split_once(',').expect("x,y");
    let x: f64 = x.parse().expect("x is a number");
    let y: f64 = y.parse().expect("y is a number");
    assert!(x >= 0.0 && y >= 0.0);

    // The same movement reached the track history the app already reads.
    let track = format!("map:{}:track:{obj}", ctx.active_id());
    let newest = ctx
        .redis_lindex(&track, 0)
        .expect("a simulated tick must land in the track history");
    let newest: serde_json::Value = serde_json::from_str(&newest).expect("track json");
    assert!((newest["lat"].as_f64().unwrap() - lat).abs() < 1e-9);
    assert_eq!(newest["spd"].as_f64().is_some(), true);

    // Leave nothing driving for the next test.
    ctx.redis_del(&simulation_key(&ctx.active_id(), obj));
}

/// Плавность: successive ticks move the object by a fraction of a cell, never by
/// a whole jump, and the coordinate it publishes carries that fraction — which is
/// what the requirement means by a smooth change of coordinates.
#[tokio::test]
async fn successive_ticks_move_the_object_by_a_fraction_of_a_cell() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();
    let (start, end) = connected_cells(&ctx, AgentKind::Engineer);
    let obj = "AT-0004";

    expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({ "obj": obj, "stops": [[start.0, start.1], [end.0, end.1]], "tick_ms": 60000 }),
        )
        .await,
        StatusCode::OK,
    );

    let map_id = ctx.active_id();
    let active = ctx.state.active.read().expect("active map").clone();
    let cell_m = active.location.cell_size_m;
    let mut redis_conn = ctx.redis();
    let mut pg_conn = ctx.db();

    let mut previous: Option<(f64, f64)> = None;
    let mut off_grid = 0;
    for _ in 0..20 {
        // A walking engineer covers ~1.4 m in half a second, far less than a cell.
        let step = crate::simulation::step(&active, &mut redis_conn, &mut pg_conn, obj, 500)
            .expect("the simulation must advance");
        let (x, y) = (step.sample.x, step.sample.y);
        if let Some((last_x, last_y)) = previous {
            let moved_m = ((x - last_x).hypot(y - last_y)) * cell_m;
            assert!(
                moved_m <= cell_m,
                "two half-second ticks must not jump a whole cell: {moved_m:.1} m of {cell_m} m"
            );
        }
        if x.fract().abs() > 1e-6 || y.fract().abs() > 1e-6 {
            off_grid += 1;
        }
        previous = Some((x, y));
    }
    assert!(
        off_grid > 10,
        "the position must be smooth between cells, not snapped: {off_grid} of 20 samples were fractional"
    );

    // The smooth coordinate the app reads is the same fraction, stored as text,
    // and it is also what the status endpoint reports.
    let smooth = ctx
        .redis_get(&smooth_key(&map_id, obj))
        .expect("smooth key");
    let (stored_x, stored_y) = smooth.split_once(',').expect("x,y");
    let stored_x: f64 = stored_x.parse().expect("x");
    let stored_y: f64 = stored_y.parse().expect("y");
    let (last_x, last_y) = previous.expect("twenty samples");
    assert!((stored_x - last_x).abs() < 1e-3 && (stored_y - last_y).abs() < 1e-3);

    let status = expect_status(
        ctx.get(
            &format!("/api/geo/simulationStatus?obj={obj}"),
            &dispatcher.token,
        )
        .await,
        StatusCode::OK,
    );
    assert!((status["x"].as_f64().unwrap() - last_x).abs() < 1e-3);
    assert!((status["y"].as_f64().unwrap() - last_y).abs() < 1e-3);

    ctx.redis_del(&simulation_key(&map_id, obj));
}

/// A faster demo covers route time faster, but it still walks the whole route:
/// the clock measures the route, so the speed multiplier must never make the
/// object arrive short of its destination.
#[tokio::test]
async fn a_speed_multiplier_shortens_the_demo_not_the_route() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();
    let (start, end) = connected_cells(&ctx, AgentKind::Engineer);
    let obj = "AT-0005";

    let body = expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({
                "obj": obj,
                "stops": [[start.0, start.1], [end.0, end.1]],
                "tick_ms": 60000,
                "speed_scale": 10.0,
            }),
        )
        .await,
        StatusCode::OK,
    );
    let total = body["seconds"].as_f64().expect("seconds");

    let map_id = ctx.active_id();
    let active = ctx.state.active.read().expect("active map").clone();
    let mut redis_conn = ctx.redis();
    let mut pg_conn = ctx.db();
    // One second of wall time is ten seconds of route at this multiplier.
    let step = crate::simulation::step(&active, &mut redis_conn, &mut pg_conn, obj, 1000)
        .expect("the simulation must advance");
    assert!(
        (total - step.sample.remaining_seconds - 10.0).abs() < 1e-6,
        "a 10x run must cover ten route seconds per second: left {} of {total}",
        step.sample.remaining_seconds
    );
    assert!(!step.sample.done, "ten seconds is not the whole journey");

    // Driving the rest of the clock reaches the end of the route, not half of it.
    let remaining = step.sample.remaining_seconds;
    let last = crate::simulation::step(
        &active,
        &mut redis_conn,
        &mut pg_conn,
        obj,
        (remaining * 1000.0) as i64,
    )
    .expect("the simulation must advance");
    assert!(last.finished, "the route must end where the plan ended");
    assert!(
        last.sample.remaining_seconds.abs() < 1e-6,
        "arriving must leave nothing to travel: {}",
        last.sample.remaining_seconds
    );
    let destination = body["points"].as_array().unwrap().last().cloned().unwrap();
    assert!((last.sample.x - destination[0].as_f64().unwrap()).abs() < 1e-6);
    assert!((last.sample.y - destination[1].as_f64().unwrap()).abs() < 1e-6);

    assert_eq!(
        ctx.redis_get(&simulation_key(&map_id, obj)),
        None,
        "a finished journey is dropped"
    );
}

/// ТЗ п.3.1: a paused agent is not moved and does not lose its route; resuming
/// continues the same journey from where it stopped.
#[tokio::test]
async fn a_paused_object_keeps_its_route_and_resumes_where_it_stopped() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();
    let (start, end) = connected_cells(&ctx, AgentKind::Engineer);
    let obj = "AT-0002";

    expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({ "obj": obj, "stops": [[start.0, start.1], [end.0, end.1]], "tick_ms": 60000 }),
        )
        .await,
        StatusCode::OK,
    );
    let map_id = ctx.active_id();
    // Pause first: a paused object is never advanced by the driver, so the clock
    // this test sets below cannot be moved under it.
    expect_status(
        ctx.post(
            "/api/geo/pauseAgent",
            &dispatcher.token,
            json!({ "obj": obj, "paused": true }),
        )
        .await,
        StatusCode::OK,
    );
    let mut redis_conn = ctx.redis();
    let mut simulation =
        crate::simulation::load(&mut redis_conn, &map_id, obj).expect("stored simulation");
    simulation.clock_ms = 0;
    crate::simulation::save(&mut redis_conn, &map_id, &simulation).expect("save");

    let active = ctx.state.active.read().expect("active map").clone();
    let mut pg_conn = ctx.db();
    let before = crate::simulation::step(&active, &mut redis_conn, &mut pg_conn, obj, 5000)
        .expect("a paused object keeps its simulation");
    assert!(!before.finished);
    let stored = crate::simulation::load(&mut redis_conn, &map_id, obj).expect("still stored");
    assert_eq!(stored.clock_ms, 0, "a paused object must not advance");
    assert!(
        stored.route.seconds > 0.0,
        "a paused object keeps its route"
    );

    let status = expect_status(
        ctx.get(
            &format!("/api/geo/simulationStatus?obj={obj}"),
            &dispatcher.token,
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(status["paused"], true);
    assert_eq!(status["active"], true);

    expect_status(
        ctx.post(
            "/api/geo/pauseAgent",
            &dispatcher.token,
            json!({ "obj": obj, "paused": false }),
        )
        .await,
        StatusCode::OK,
    );
    let after = crate::simulation::step(&active, &mut redis_conn, &mut pg_conn, obj, 5000)
        .expect("resumed");
    let resumed = crate::simulation::load(&mut redis_conn, &map_id, obj).expect("still stored");
    assert!(resumed.clock_ms > 0, "resuming must move the object again");
    assert!(after.sample.remaining_seconds < before.sample.remaining_seconds);

    ctx.redis_del(&simulation_key(&map_id, obj));
}

/// Stopping ends the movement but keeps the position reached, and it is in the
/// journal too.
#[tokio::test]
async fn stopping_a_movement_keeps_the_position_and_clears_the_route() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();
    let (start, end) = connected_cells(&ctx, AgentKind::Vehicle);
    let obj = "vehicle-01";

    expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({ "obj": obj, "stops": [[start.0, start.1], [end.0, end.1]], "tick_ms": 60000 }),
        )
        .await,
        StatusCode::OK,
    );
    let moving = expect_status(
        ctx.get(
            &format!("/api/geo/simulationStatus?obj={obj}"),
            &dispatcher.token,
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(moving["active"], true);

    let stopped = expect_status(
        ctx.post(
            "/api/geo/stopSimulation",
            &dispatcher.token,
            json!({ "obj": obj }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(stopped["stopped"], true);
    assert!(
        stopped["point"]["x"].is_number(),
        "last cell kept: {stopped}"
    );

    let after = expect_status(
        ctx.get(
            &format!("/api/geo/simulationStatus?obj={obj}"),
            &dispatcher.token,
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(after["active"], false);
    assert!(after["x"].is_number() && after["y"].is_number());

    let mut conn = ctx.db();
    let stopped_events: i64 = crate::database::schema::journal::table
        .filter(crate::database::schema::journal::kind.eq("simulation_stopped"))
        .count()
        .get_result(&mut conn)
        .expect("journal count");
    assert_eq!(stopped_events, 1, "a manual stop must be journalled");

    // Stopping again is not an error, it just reports that nothing was running.
    let again = expect_status(
        ctx.post(
            "/api/geo/stopSimulation",
            &dispatcher.token,
            json!({ "obj": obj }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(again["stopped"], false);
}

/// Only a dispatcher may move an object, and a request that cannot produce a
/// movement is refused before anything is stored.
#[tokio::test]
async fn refuses_movement_it_cannot_plan() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let (start, end) = connected_cells(&ctx, AgentKind::Engineer);

    expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &engineer.token,
            json!({ "obj": "AT-0003", "stops": [[start.0, start.1], [end.0, end.1]] }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );

    // An object neither the map nor the id shapes know: the movement would be
    // published into nothing.
    expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({ "obj": "nobody-at-all", "stops": [[start.0, start.1], [end.0, end.1]] }),
        )
        .await,
        StatusCode::NOT_FOUND,
    );

    // Fewer than two stops is not a route.
    expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({ "obj": "AT-0003", "stops": [[start.0, start.1]] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    // The same cell twice is nothing to move.
    expect_status(
        ctx.post(
            "/api/geo/startSimulation",
            &dispatcher.token,
            json!({ "obj": "AT-0003", "stops": [[start.0, start.1], [start.0, start.1]] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    // Nothing was left behind by the refusals.
    assert_eq!(
        ctx.redis_get(&simulation_key(&ctx.active_id(), "AT-0003")),
        None
    );
    let mut conn = ctx.db();
    let started: i64 = crate::database::schema::journal::table
        .filter(crate::database::schema::journal::kind.eq("simulation_started"))
        .count()
        .get_result(&mut conn)
        .expect("journal count");
    assert_eq!(started, 0);
}
