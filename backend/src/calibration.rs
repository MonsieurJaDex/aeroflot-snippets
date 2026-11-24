//! Manual coordinate calibration of tracked objects.
//!
//! A tracker reports raw WGS-84, and the graph places the object by snapping
//! that point to the nearest pavement cell. When the snap keeps landing in the
//! wrong place — a phone drifting near a hangar, an antenna offset — GNSS is
//! not going to improve, so the operator pins the object to the cell where it
//! actually is. The pin is stored next to the active map
//! (`map:{id}:calibration:{obj}`), so switching maps does not carry a pin from
//! one grid onto another.

use anyhow::Context;
use redis::TypedCommands;
use serde::{Deserialize, Serialize};

use crate::types::{enums::ObjectKind, map::Point};

/// A pinned cell for one object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub obj: String,
    pub kind: ObjectKind,
    pub point: Point,
}

/// `map:{id}:calibration:{obj}`.
#[must_use]
pub fn key(map_id: &str, obj: &str) -> String {
    format!("map:{map_id}:calibration:{obj}")
}

/// `map:{id}:calibration:*`.
#[must_use]
pub fn scan(map_id: &str) -> String {
    format!("map:{map_id}:calibration:*")
}

/// Reads the pin of `obj`, or `None` when the object is uncalibrated.
///
/// A stored value that does not parse is reported and treated as absent: a
/// broken pin must not stop a tracker from being ingested.
#[must_use]
pub fn load(conn: &mut redis::Connection, map_id: &str, obj: &str) -> Option<Calibration> {
    let key = key(map_id, obj);
    let raw: Option<String> = match conn.get(&key) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%obj, %error, "failed to read object calibration");
            return None;
        }
    };
    let raw = raw?;
    serde_json::from_str(&raw).ok().or_else(|| {
        tracing::warn!(%obj, "object calibration does not parse, ignored");
        None
    })
}

/// Stores a pin for `obj`.
pub fn save(
    conn: &mut redis::Connection,
    map_id: &str,
    calibration: &Calibration,
) -> anyhow::Result<()> {
    let value = serde_json::to_string(calibration).context("failed to encode calibration")?;
    conn.set(key(map_id, &calibration.obj), value)
        .context("failed to store calibration")
}

/// Removes the pin of `obj`, returning whether one was stored.
pub fn clear(conn: &mut redis::Connection, map_id: &str, obj: &str) -> anyhow::Result<bool> {
    let removed: i64 = redis::cmd("DEL")
        .arg(key(map_id, obj))
        .query(conn)
        .context("failed to drop calibration")?;
    Ok(removed > 0)
}

/// Every pin stored for the active map.
pub fn list(conn: &mut redis::Connection, map_id: &str) -> anyhow::Result<Vec<Calibration>> {
    let keys: Vec<String> = conn
        .scan_match(scan(map_id))
        .context("failed to scan calibrations")?
        .filter_map(Result::ok)
        .collect();
    let mut result = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(raw) = conn.get(&key).context("failed to read calibration")? else {
            continue;
        };
        match serde_json::from_str::<Calibration>(&raw) {
            Ok(calibration) => result.push(calibration),
            Err(error) => tracing::warn!(%key, %error, "skipped an unreadable calibration"),
        }
    }
    result.sort_by(|left, right| left.obj.cmp(&right.obj));
    Ok(result)
}
