//! Facts about the aircraft a task belongs to, as ranking needs them.
//!
//! Assignment is scored per engineer, but two of its inputs belong to the
//! aircraft rather than to the task or to the engineer:
//!
//! * when the aircraft last arrived on the active map — read from
//!   `aircraft_movements`, the open stay only. `tasks.arrived_at` is a
//!   different moment (the engineer reaching the aircraft) and is never read
//!   here;
//! * whether the aircraft is AOG — the flag the engineer sets on their active
//!   task, kept in Redis under `map:{id}:aog:{uuid}` and removed when the
//!   aircraft departs.
//!
//! Both live outside the request handler, so one loader answers both and the
//! scorer stays free of connections.

use anyhow::Context;
use chrono::{DateTime, Utc};
use diesel::{PgConnection, prelude::*};
use redis::TypedCommands;
use uuid::Uuid;

use crate::{database::schema::aircraft_movements, maps::ActiveMap};

/// What ranking knows about the aircraft behind a task.
#[derive(Debug, Clone, Copy, Default)]
pub struct AircraftContext {
    /// The aircraft is on the active map right now, arrived at this moment.
    /// `None` when it never arrived on this map or has already departed.
    pub arrived_at: Option<DateTime<Utc>>,
    /// The engineer flagged the aircraft as AOG (`aog:{uuid}` exists).
    pub aog: bool,
}

/// Reads the arrival and the AOG flag of `aircraft_id` on the active map.
///
/// Errors from Postgres or Redis are returned, not swallowed: a missing flag
/// and an unreadable Redis must not look the same to the caller.
pub fn aircraft_context(
    conn: &mut PgConnection,
    redis_conn: &mut redis::Connection,
    map: &ActiveMap,
    aircraft_id: Uuid,
) -> anyhow::Result<AircraftContext> {
    Ok(AircraftContext {
        arrived_at: last_arrival(conn, map, aircraft_id)?,
        aog: redis_conn
            .exists(map.aog_key(aircraft_id))
            .context("failed to read the AOG flag of the aircraft")?,
    })
}

/// `aircraft_movements` row of `aircraft_id` on `map`, latest first by
/// `(arrived_at, id)` so two rows of the same instant still resolve
/// deterministically. A departed aircraft reports `None`: it is not here.
fn last_arrival(
    conn: &mut PgConnection,
    map: &ActiveMap,
    aircraft_id: Uuid,
) -> anyhow::Result<Option<DateTime<Utc>>> {
    let stay = aircraft_movements::table
        .filter(aircraft_movements::aircraft_id.eq(aircraft_id))
        .filter(aircraft_movements::map_id.eq(&map.id))
        .select((
            aircraft_movements::arrived_at,
            aircraft_movements::departed_at,
        ))
        .order_by(aircraft_movements::arrived_at.desc())
        .then_order_by(aircraft_movements::id.desc())
        .first::<(DateTime<Utc>, Option<DateTime<Utc>>)>(conn)
        .optional()
        .context("failed to read the last movement of the aircraft")?;

    Ok(stay.and_then(|(arrived_at, departed_at)| departed_at.is_none().then_some(arrived_at)))
}
