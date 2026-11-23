//! Catalog of airport maps and the one map the process is serving.
//!
//! Positions, AOG flags and cached routes in Redis are stored under
//! `map:{id}:…`, so switching the active map does not mix coordinates
//! from another grid. Auth keys stay global.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use redis::TypedCommands;
use uuid::Uuid;

use crate::{
    parser,
    types::{enums::VehicleType, map::MapMatrix},
};

const ACTIVE_KEY: &str = "map:active";
const LEGACY_MOVED_KEY: &str = "map:legacy_keys_moved";

/// A map file the server can switch to.
#[derive(Debug, Clone)]
pub struct MapCatalogEntry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub width: usize,
    pub height: usize,
}

/// Maps discovered next to the CLI map file.
#[derive(Debug, Clone)]
pub struct MapCatalog {
    pub entries: Vec<MapCatalogEntry>,
}

/// Matrix currently used by routing and bounds checks.
#[derive(Debug, Clone)]
pub struct ActiveMap {
    pub id: String,
    pub name: String,
    pub map: MapMatrix,
    pub road_points: HashSet<i64>,
    pub loaded_at: DateTime<Utc>,
}

impl ActiveMap {
    pub fn engineer_position_key(&self, id: Uuid) -> String {
        format!("map:{}:position:{id}", self.id)
    }

    pub fn engineer_timestamp_key(&self, id: Uuid) -> String {
        format!("map:{}:position_ts:{id}", self.id)
    }

    pub fn vehicle_position_key(&self, vehicle: &VehicleType, id: Uuid) -> String {
        format!("map:{}:position:{vehicle}:{id}", self.id)
    }

    pub fn vehicle_scan(&self, vehicle: &str) -> String {
        format!("map:{}:position:{vehicle}:*", self.id)
    }

    pub fn aog_key(&self, id: Uuid) -> String {
        format!("map:{}:aog:{id}", self.id)
    }

    pub fn aog_scan(&self) -> String {
        format!("map:{}:aog:*", self.id)
    }

    /// `map:{id}:aog:{uuid}` → uuid.
    pub fn aog_id_from_key<'a>(&self, key: &'a str) -> Option<&'a str> {
        key.strip_prefix(&format!("map:{}:aog:", self.id))
    }

    /// `map:{id}:position:aircraft:{uuid}` → uuid.
    pub fn aircraft_id_from_key<'a>(&self, key: &'a str) -> Option<&'a str> {
        key.strip_prefix(&format!("map:{}:position:aircraft:", self.id))
    }
}

impl MapCatalog {
    pub fn get(&self, id: &str) -> Option<&MapCatalogEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}

/// Loads every `*.json` map in the directory of `map_path`, and that file itself.
pub fn load_catalog(map_path: &str) -> anyhow::Result<(MapCatalog, String)> {
    let cli_path = PathBuf::from(map_path);
    let cli_id = map_id_from_path(&cli_path)?;
    let dir = cli_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("failed to read map directory {}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    if !paths.iter().any(|path| path == &cli_path) {
        paths.push(cli_path);
    }
    paths.sort();

    let mut entries = Vec::new();
    for path in paths {
        let id = match map_id_from_path(&path) {
            Ok(id) => id,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "skipped map file");
                continue;
            }
        };
        if entries.iter().any(|entry: &MapCatalogEntry| entry.id == id) {
            bail!("two map files share the id '{id}'");
        }
        let parsed = parser::parse_from_json(&path.to_string_lossy())
            .map_err(|error| anyhow::anyhow!("{error}"))
            .with_context(|| format!("failed to parse map {} ({id})", path.display()))?;
        entries.push(MapCatalogEntry {
            id,
            name: path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("map")
                .to_owned(),
            width: parsed.map.width(),
            height: parsed.map.height(),
            path,
        });
    }
    if entries.is_empty() {
        bail!("no airport maps found next to {map_path}");
    }
    if !entries.iter().any(|entry| entry.id == cli_id) {
        bail!("cli map '{cli_id}' was not loaded");
    }
    Ok((MapCatalog { entries }, cli_id))
}

pub fn load_active(entry: &MapCatalogEntry) -> anyhow::Result<ActiveMap> {
    let parsed = parser::parse_from_json(&entry.path.to_string_lossy())
        .map_err(|error| anyhow::anyhow!("{error}"))
        .with_context(|| format!("failed to parse map {}", entry.path.display()))?;
    Ok(ActiveMap {
        id: entry.id.clone(),
        name: entry.name.clone(),
        map: parsed.map,
        road_points: parsed.road.into_iter().collect(),
        loaded_at: Utc::now(),
    })
}

/// Uses `map:active` when it names a known file, otherwise the CLI map.
pub fn resolve_active(
    redis_conn: &mut redis::Connection,
    catalog: &MapCatalog,
    cli_id: &str,
) -> anyhow::Result<ActiveMap> {
    let stored: Option<String> = redis_conn
        .get(ACTIVE_KEY)
        .context("failed to read the active map id")?;
    let id = match stored {
        Some(id) if catalog.get(&id).is_some() => id,
        Some(id) => {
            tracing::warn!(%id, fallback = cli_id, "stored active map is not in the catalog");
            cli_id.to_owned()
        }
        None => cli_id.to_owned(),
    };
    let entry = catalog
        .get(&id)
        .expect("active map id was checked against the catalog");
    let active = load_active(entry)?;
    redis_conn
        .set(ACTIVE_KEY, &active.id)
        .context("failed to store the active map id")?;
    migrate_legacy_keys(redis_conn, &active.id)?;
    Ok(active)
}

pub fn store_active(redis_conn: &mut redis::Connection, id: &str) -> anyhow::Result<()> {
    redis_conn
        .set(ACTIVE_KEY, id)
        .context("failed to store the active map id")?;
    Ok(())
}

/// Moves pre-map keys (`position:…`, `aog:…`) under the map that is active
/// the first time the server starts with this layout.
fn migrate_legacy_keys(redis_conn: &mut redis::Connection, map_id: &str) -> anyhow::Result<()> {
    let moved: Option<String> = redis_conn
        .get(LEGACY_MOVED_KEY)
        .context("failed to read the legacy-key marker")?;
    if moved.is_some() {
        return Ok(());
    }
    for prefix in ["position:", "position_ts:", "aog:"] {
        let keys: Vec<String> = redis_conn
            .scan_match(format!("{prefix}*"))
            .context("failed to scan legacy redis keys")?
            .filter_map(Result::ok)
            .collect();
        for key in keys {
            if key.starts_with("map:") {
                continue;
            }
            let dest = format!("map:{map_id}:{key}");
            if redis_conn
                .exists(&dest)
                .context("failed to check a namespaced redis key")?
            {
                continue;
            }
            if let Err(error) = redis::cmd("RENAME")
                .arg(&key)
                .arg(&dest)
                .query::<()>(redis_conn)
            {
                tracing::warn!(%key, %dest, %error, "legacy redis key was not renamed");
            }
        }
    }
    redis_conn
        .set(LEGACY_MOVED_KEY, map_id)
        .context("failed to mark legacy redis keys as moved")?;
    Ok(())
}

pub fn route_cache_key(map_id: &str, hash: u64) -> String {
    format!("map:{map_id}:route:{hash}")
}

fn map_id_from_path(path: &Path) -> anyhow::Result<String> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .context("map file name is not valid utf-8")?;
    if stem.is_empty()
        || !stem
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        bail!("map id '{stem}' may only contain letters, digits, '_' and '-'");
    }
    Ok(stem.to_owned())
}

/// Shared handle kept in [`crate::types::config::AppState`].
pub type SharedMap = Arc<std::sync::RwLock<ActiveMap>>;
