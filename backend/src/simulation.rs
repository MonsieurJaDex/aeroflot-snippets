//! Simulation of an object moving along a planned route.
//!
//! The graph plans a route as timed, speed-classed segments ([`crate::graph::RouteSegment`]);
//! this module turns that plan into a position over time. A route is not walked
//! cell by cell — the object is placed at the point a fraction of the way along
//! the segment that covers the elapsed time, so its coordinates change smoothly
//! instead of jumping a whole cell every step.
//!
//! # Where the state lives
//!
//! `map:{id}:sim:{obj}` holds one JSON document per moving object: the route, the
//! distance already driven (as route time) and the speed multiplier. Redis is the
//! only place it lives, so the driver in another process, a resumed server or a
//! test all see the same movement. The clock is route time, not wall time: a
//! tick advances it, and a pause simply stops advancing it (ТЗ п.3.1 — stop the
//! agent without ending its route).
//!
//! # How a position reaches the map
//!
//! [`step`] hands the sample to `router::track`, the same path a tracker report
//! takes (`store_point` for history, `publish_position` for the coordinate keys),
//! minus the two ingest gates a simulator would fail: the staleness check reads
//! the wall clock and the outlier guard caps speed at the airfield limit, so a
//! 500 ms tick would look like a jump and be dropped. Everything else — the
//! object's kind, its position key, the engineer's timestamp and the SLA
//! forecast — stays exactly what a real tracker produces.

use std::sync::Arc;

use chrono::Utc;
use diesel::PgConnection;
use redis::TypedCommands;
use serde::{Deserialize, Serialize};

use crate::graph::PlannedRoute;
use crate::maps::ActiveMap;
use crate::types::config::AppState;
use crate::types::map::Point;

/// How often the driver advances a running simulation, in milliseconds.
///
/// Half a second is what makes the movement look continuous on the dispatcher
/// map while keeping the per-object cost trivial.
pub const DEFAULT_TICK_MS: i64 = 500;

/// Position of a route at one instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimSample {
    /// Fractional cell the object is at. `project_f64` turns it into degrees.
    pub x: f64,
    pub y: f64,
    /// Whether the route is complete at this instant.
    pub done: bool,
    /// Route seconds still to travel.
    pub remaining_seconds: f64,
}

/// The point of `route` at `elapsed` seconds, interpolated along the segment that
/// covers that time.
///
/// The time of a segment comes from its own speed class, so a route that crosses
/// a runway is covered in proportion to how fast the graph said that piece is
/// walked. `elapsed` outside the route is clamped to its ends.
#[must_use]
pub fn sample_at(route: &PlannedRoute, elapsed: f64) -> SimSample {
    let total = route.seconds.max(0.0);
    let elapsed = elapsed.max(0.0);
    // The end is compared in the units the clock is stored in: whole milliseconds.
    // A route rarely lasts a whole number of them (448.2 s here), and comparing
    // 448.2 s against 448.20000000000086 s would leave a finished route running
    // forever — the object parked on its last cell, the journal never closed.
    let done = (elapsed * 1000.0).round() >= (total * 1000.0).round();
    if route.segments.is_empty() {
        // A route with fewer than two points: the object stays where it is.
        let start = route.points.first().copied().unwrap_or(Point::new(0, 0));
        return SimSample {
            x: start.0 as f64,
            y: start.1 as f64,
            done: true,
            remaining_seconds: 0.0,
        };
    }
    // The segment covering `elapsed`: the last one that ends after it. A zero
    // duration segment is never "covering" anything, so it cannot divide by zero.
    let mut index = route.segments.len() - 1;
    for (position, segment) in route.segments.iter().enumerate() {
        if segment.cum_seconds >= elapsed {
            index = position;
            break;
        }
    }
    let segment = &route.segments[index];
    let start_of_segment = segment.cum_seconds - segment.seconds;
    let fraction = if segment.seconds > f64::MIN_POSITIVE {
        ((elapsed - start_of_segment) / segment.seconds).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let x = segment.from.0 as f64 + (segment.to.0 as f64 - segment.from.0 as f64) * fraction;
    let y = segment.from.1 as f64 + (segment.to.1 as f64 - segment.from.1 as f64) * fraction;
    SimSample {
        x,
        y,
        done,
        remaining_seconds: if done {
            0.0
        } else {
            (total - elapsed).max(0.0)
        },
    }
}

/// One stored simulation: the route plus how far along it the object is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Simulation {
    /// Object the route belongs to, exactly as the tracker would name it.
    pub obj: String,
    /// The planned route with its per-segment speeds.
    pub route: PlannedRoute,
    /// Route time already driven, in milliseconds. Advance it, and the object
    /// moves; leave it, and the object stands still with its route intact.
    pub clock_ms: i64,
    /// Wall-clock instant the movement started, for the journal and the answer.
    pub started_at_ms: i64,
    /// Multiplier over the route's own speeds. `1.0` is the graph's timing.
    pub speed_scale: f64,
    /// Tick the driver runs at, in milliseconds.
    pub tick_ms: i64,
    /// Identifies the driver loop that owns this state. A second
    /// `startSimulation` on the same object overwrites the state and therefore
    /// the run: the older loop sees a different token on its next tick and stops
    /// instead of driving the same clock twice.
    #[serde(default)]
    pub run: String,
}

impl Simulation {
    /// Elapsed route seconds at the stored clock.
    #[must_use]
    pub fn elapsed_seconds(&self) -> f64 {
        self.clock_ms.max(0) as f64 / 1000.0
    }

    /// Where the object is now.
    #[must_use]
    pub fn sample(&self) -> SimSample {
        sample_at(&self.route, self.elapsed_seconds())
    }
}

/// Key holding one object's running simulation.
#[must_use]
pub fn simulation_key(map_id: &str, obj: &str) -> String {
    format!("map:{map_id}:sim:{obj}")
}

/// Key holding the fractional cell of a simulated object — the smooth coordinate,
/// as `"x,y"` with decimals, which the integer `Point` keys cannot carry.
#[must_use]
pub fn smooth_key(map_id: &str, obj: &str) -> String {
    format!("map:{map_id}:smooth:{obj}")
}

/// Reads a stored simulation, or `None` when the object is not moving.
#[must_use]
pub fn load(redis_conn: &mut redis::Connection, map_id: &str, obj: &str) -> Option<Simulation> {
    let raw = redis_conn.get(simulation_key(map_id, obj)).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

/// Stores a simulation. The key outlives a long route but not forever: a route
/// that stops being ticked is dropped rather than replayed hours later.
pub fn save(
    redis_conn: &mut redis::Connection,
    map_id: &str,
    simulation: &Simulation,
) -> redis::RedisResult<()> {
    let key = simulation_key(map_id, &simulation.obj);
    let ttl = (simulation.route.seconds / simulation.speed_scale.max(0.01)) as i64 + 3600;
    redis_conn.set(&key, serde_json::to_string(simulation).unwrap_or_default())?;
    redis_conn.expire(&key, ttl.max(60)).map(|_: bool| ())
}

/// Removes a simulation; the object keeps its last published position.
pub fn clear(
    redis_conn: &mut redis::Connection,
    map_id: &str,
    obj: &str,
) -> redis::RedisResult<()> {
    redis_conn
        .del(simulation_key(map_id, obj))
        .map(|_: usize| ())
}

/// What one [`step`] did.
#[derive(Debug, Clone, Copy)]
pub struct Step {
    /// Position after the step, as a fractional cell.
    pub sample: SimSample,
    /// Whether the route is complete and the simulation was removed.
    pub finished: bool,
}

/// Advances a running simulation by the time that really passed and publishes the
/// new position.
///
/// `elapsed_ms` is wall time since the previous step, not the tick's own length:
/// a driver that stalls for two seconds must move the object two seconds' worth,
/// and a tick that arrives early must not run the journey faster than the graph
/// timed it. `speed_scale` is what makes a demo faster, explicitly.
///
/// A paused object ([`crate::router::track::is_paused`]) is not advanced at all:
/// its route and its clock stay where they were, so resuming continues the
/// journey instead of restarting it.
pub fn step(
    active: &ActiveMap,
    redis_conn: &mut redis::Connection,
    pg_conn: &mut PgConnection,
    obj: &str,
    elapsed_ms: i64,
) -> Option<Step> {
    let mut simulation = load(redis_conn, &active.id, obj)?;
    if crate::router::track::is_paused(redis_conn, &active.id, obj) {
        let sample = simulation.sample();
        return Some(Step {
            sample,
            finished: false,
        });
    }
    let advance = (elapsed_ms.max(0) as f64 * simulation.speed_scale.max(0.01)).round() as i64;
    // `clock_ms` is route time: the ceiling is the route's own length, never a
    // scaled one, or a faster demo would arrive half-way along its journey.
    simulation.clock_ms =
        (simulation.clock_ms + advance).min((simulation.route.seconds * 1000.0).round() as i64);
    let sample = simulation.sample();
    publish(active, redis_conn, pg_conn, &simulation, sample);
    if sample.done {
        let _ = clear(redis_conn, &active.id, obj);
        return Some(Step {
            sample,
            finished: true,
        });
    }
    if let Err(error) = save(redis_conn, &active.id, &simulation) {
        tracing::error!(%error, %obj, "failed to store simulation state");
    }
    Some(Step {
        sample,
        finished: false,
    })
}

/// Publishes one sample: the smooth coordinate, then the position the map reads.
fn publish(
    active: &ActiveMap,
    redis_conn: &mut redis::Connection,
    pg_conn: &mut PgConnection,
    simulation: &Simulation,
    sample: SimSample,
) {
    let (lat, lon) = active.location.graph.project_f64(sample.x, sample.y);
    let smooth = format!("{:.4},{:.4}", sample.x, sample.y);
    let _ = redis_conn.set(smooth_key(&active.id, &simulation.obj), smooth);
    let speed = current_speed_mps(&simulation.route, simulation.elapsed_seconds());
    crate::router::track::publish_simulated_point(
        active,
        redis_conn,
        pg_conn,
        &simulation.obj,
        lat,
        lon,
        Utc::now(),
        speed,
        Some((sample.x, sample.y)),
    );
}

/// Speed of the route at `elapsed`, so a simulated point reports the speed a
/// tracker would have measured rather than nothing.
fn current_speed_mps(route: &PlannedRoute, elapsed: f64) -> Option<f64> {
    let segment = route
        .segments
        .iter()
        .find(|segment| segment.cum_seconds >= elapsed)?;
    Some(segment.speed_mps)
}

/// Drives one simulation until its route is complete or the state disappears.
///
/// The loop re-reads the state every tick: that is what makes a pause, a resume
/// and a `stopSimulation` from any request take effect on the next tick, and it
/// means a restarted process picks the movement up where it stopped.
pub async fn drive(app_state: Arc<AppState>, obj: String) {
    let (tick_ms, run) = {
        let active = match app_state.active.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        match load_maybe(&app_state, &active.id, &obj) {
            Some(simulation) => (simulation.tick_ms.max(1), simulation.run),
            None => return,
        }
    };
    let mut last = std::time::Instant::now();
    loop {
        let passed = last.elapsed().as_millis().min(i64::MAX as u128) as i64;
        last = std::time::Instant::now();
        let active = match app_state.active.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        let (mut redis_conn, mut pg_conn) =
            match (app_state.redis_pool.get(), app_state.db_pool.get()) {
                (Ok(redis_conn), Ok(pg_conn)) => (redis_conn, pg_conn),
                _ => {
                    tracing::error!(%obj, "simulation driver lost its connections");
                    return;
                }
            };
        // A newer start on this object owns the clock now; this loop is stale.
        if load(&mut redis_conn, &active.id, &obj).is_some_and(|current| current.run != run) {
            return;
        }
        let Some(step) = step(&active, &mut redis_conn, &mut pg_conn, &obj, passed) else {
            return;
        };
        if step.finished {
            // The journey ended on its own: the journal says where it arrived.
            crate::journal::record(
                &mut pg_conn,
                "simulation_finished",
                Some(&obj),
                None,
                serde_json::json!({
                    "obj": obj,
                    "x": step.sample.x,
                    "y": step.sample.y,
                }),
                None,
            );
            return;
        }
        drop((redis_conn, pg_conn));
        tokio::time::sleep(std::time::Duration::from_millis(tick_ms as u64)).await;
    }
}

fn load_maybe(app_state: &AppState, map_id: &str, obj: &str) -> Option<Simulation> {
    let mut conn = app_state.redis_pool.get().ok()?;
    load(&mut conn, map_id, obj)
}

/// Agent kind of an object id, following the shapes the tracker already uses:
/// `RA-…` is an aircraft, `vehicle-…`/`v-…` a vehicle, everything else walks.
#[must_use]
pub fn agent_of(obj: &str) -> crate::graph::AgentKind {
    // An aircraft is not a pedestrian: it moves on the vehicle side of the graph,
    // where the apron and runway speeds live. Only an engineer walks.
    if obj.starts_with("vehicle-") || obj.starts_with("v-") || obj.starts_with("RA-") {
        crate::graph::AgentKind::Vehicle
    } else {
        crate::graph::AgentKind::Engineer
    }
}
