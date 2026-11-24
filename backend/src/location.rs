//! Location package: speeds, stand zones, ATA chapters and external agent ids.
//!
//! The file `{map_id}.location.json` next to the map overrides the defaults.
//! Switching polygons is a config change.

use std::{collections::HashSet, fs, path::Path};

use serde::Deserialize;

use crate::{
    graph::{graph_from_grid_with_overlays, RoadGraph, SpeedClass},
    types::map::MapDocument,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Zone {
    #[allow(dead_code)]
    pub id: String,
    pub x: i64,
    pub y: i64,
    pub radius_m: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MapObject {
    #[allow(dead_code)]
    pub id: String,
    pub kind: String,
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AtaChapter {
    pub chapter: String,
    #[allow(dead_code)]
    pub title: String,
    pub admissions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExternalAgent {
    pub obj: String,
    pub kind: String,
    #[serde(default)]
    pub engineer_id: Option<String>,
    #[serde(default)]
    pub vehicle_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LocationFile {
    #[serde(default = "default_cell")]
    cell_size_m: f64,
    #[serde(default = "default_lat")]
    origin_lat: f64,
    #[serde(default = "default_lon")]
    origin_lon: f64,
    #[serde(default = "default_sla")]
    sla_secs: u64,
    #[serde(default = "default_alert")]
    alert_before_secs: u64,
    #[serde(default = "default_boarding")]
    boarding_delay_secs: f64,
    #[serde(default = "default_tool")]
    tool_delay_secs: f64,
    #[serde(default = "default_accuracy")]
    max_accuracy_m: f64,
    #[serde(default = "default_speed")]
    max_speed_mps: f64,
    #[serde(default = "default_stale")]
    stale_after_secs: i64,
    #[serde(default = "default_aog_bonus")]
    aog_bonus_secs: f64,
    #[serde(default = "default_arrival_bonus")]
    arrival_bonus_secs: f64,
    #[serde(default = "default_arrival_window")]
    arrival_window_secs: u64,
    /// Seconds knocked off the score of an `urgent` request.
    #[serde(default = "default_urgency_bonus")]
    urgency_bonus_secs: f64,
    /// Base of the MEL bonus scale (MEL category B). A/C/D scale around it.
    #[serde(default = "default_mel_bonus")]
    mel_bonus_secs: f64,
    #[serde(default)]
    zones: Vec<Zone>,
    #[serde(default)]
    objects: Vec<MapObject>,
    #[serde(default)]
    ata: Vec<AtaChapter>,
    #[serde(default)]
    agents: Vec<ExternalAgent>,
    #[serde(default)]
    closed_cells: Vec<[i64; 2]>,
}

fn default_cell() -> f64 {
    5.0
}
fn default_lat() -> f64 {
    55.831
}
fn default_lon() -> f64 {
    37.636
}
fn default_sla() -> u64 {
    900
}
fn default_alert() -> u64 {
    300
}
fn default_boarding() -> f64 {
    10.0
}
fn default_tool() -> f64 {
    30.0
}
fn default_accuracy() -> f64 {
    80.0
}
fn default_speed() -> f64 {
    12.0
}
fn default_stale() -> i64 {
    45
}
fn default_aog_bonus() -> f64 {
    120.0
}
fn default_arrival_bonus() -> f64 {
    90.0
}
fn default_arrival_window() -> u64 {
    3600
}
fn default_urgency_bonus() -> f64 {
    60.0
}
fn default_mel_bonus() -> f64 {
    90.0
}

#[derive(Debug, Clone)]
pub struct Location {
    pub cell_size_m: f64,
    pub sla_secs: u64,
    pub alert_before_secs: u64,
    pub boarding_delay_secs: f64,
    pub tool_delay_secs: f64,
    /// Seconds knocked off the score of every candidate when the aircraft is
    /// AOG. Halved for a request that does not need a warehouse stop.
    pub aog_bonus_secs: f64,
    /// Full bonus (seconds) for an aircraft that has just arrived; it fades
    /// linearly to zero over [`Location::arrival_window_secs`].
    pub arrival_bonus_secs: f64,
    /// Width of the post-arrival window in seconds.
    pub arrival_window_secs: u64,
    /// Seconds knocked off the score of an `urgent` (not `normal`) request.
    pub urgency_bonus_secs: f64,
    /// Base of the MEL bonus scale, keyed like MEL category B; A/C/D scale
    /// around it. `None` carries no bonus.
    pub mel_bonus_secs: f64,
    pub max_accuracy_m: f64,
    pub max_speed_mps: f64,
    #[allow(dead_code)]
    pub stale_after_secs: i64,
    pub zones: Vec<Zone>,
    pub objects: Vec<MapObject>,
    pub ata: Vec<AtaChapter>,
    pub agents: Vec<ExternalAgent>,
    pub graph: RoadGraph,
}

impl Location {
    pub fn zone_contains(&self, zone: &Zone, x: i64, y: i64) -> bool {
        let dx = (x - zone.x) as f64 * self.cell_size_m;
        let dy = (y - zone.y) as f64 * self.cell_size_m;
        dx * dx + dy * dy <= zone.radius_m * zone.radius_m
    }

    pub fn object_point(&self, kind: &str) -> Option<crate::types::map::Point> {
        self.objects
            .iter()
            .find(|object| object.kind == kind)
            .map(|object| crate::types::map::Point::new(object.x, object.y))
    }
}

/// Builds the runtime view of a map: knobs from `{map_id}.location.json`, and
/// geometry from the map document itself.
///
/// The sidecar wins where it states a value (deployment tuning); the map
/// document supplies anything it omits, so a v2 map is self-describing. The
/// plausibility ceiling is `max(sidecar.max_speed_mps, palette maximum)`: a
/// 12 m/s sidecar must not drop a legal 300 km/h runway movement.
pub fn load_location(map_path: &Path, document: &MapDocument) -> Location {
    let sidecar = map_path.with_file_name(format!(
        "{}.location.json",
        map_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("map")
    ));
    let file = fs::read_to_string(&sidecar)
        .ok()
        .and_then(|raw| serde_json::from_str::<LocationFile>(&raw).ok())
        .unwrap_or(LocationFile {
            cell_size_m: document.cell_size_m,
            origin_lat: document.origin.lat,
            origin_lon: document.origin.lon,
            sla_secs: default_sla(),
            alert_before_secs: default_alert(),
            boarding_delay_secs: default_boarding(),
            tool_delay_secs: default_tool(),
            max_accuracy_m: default_accuracy(),
            max_speed_mps: default_speed(),
            stale_after_secs: default_stale(),
            aog_bonus_secs: default_aog_bonus(),
            arrival_bonus_secs: default_arrival_bonus(),
            arrival_window_secs: default_arrival_window(),
            urgency_bonus_secs: default_urgency_bonus(),
            mel_bonus_secs: default_mel_bonus(),
            zones: Vec::new(),
            objects: Vec::new(),
            ata: Vec::new(),
            agents: Vec::new(),
            closed_cells: Vec::new(),
        });
    let mut closed: HashSet<(i64, i64)> = file
        .closed_cells
        .iter()
        .map(|pair| (pair[0], pair[1]))
        .collect();
    closed.extend(document.closed_cells.iter().map(|pair| (pair[0], pair[1])));
    let ata = if file.ata.is_empty() {
        default_ata()
    } else {
        file.ata
    };

    // Geometry: a v2 map states its own grid and origin, so it wins. A v1 map
    // (`name == "legacy"`) carries no binding at all — the loader synthesises a
    // placeholder origin — so the sidecar decides there.
    let legacy_map = document.name == "legacy";
    let cell_size_m = if legacy_map || document.cell_size_m <= 0.0 {
        if legacy_map || file.cell_size_m > 0.0 {
            file.cell_size_m.max(f64::MIN_POSITIVE)
        } else {
            document.cell_size_m
        }
    } else {
        if (file.cell_size_m - document.cell_size_m).abs() > f64::EPSILON && file.cell_size_m > 0.0
        {
            tracing::warn!(
                map_cell_size_m = document.cell_size_m,
                sidecar_cell_size_m = file.cell_size_m,
                "map and sidecar disagree on cell size; using the map"
            );
        }
        document.cell_size_m
    };
    let (origin_lat, origin_lon) = if legacy_map {
        (file.origin_lat, file.origin_lon)
    } else {
        (document.origin.lat, document.origin.lon)
    };

    let road_of = |value: i64| {
        document
            .entry(value)
            .filter(|entry| entry.allows_road())
            .and_then(|entry| entry.speed_class.as_deref())
            .map(SpeedClass::from_name)
    };
    let walk_of = |value: i64| {
        document
            .entry(value)
            .filter(|entry| entry.allows_walk())
            .and_then(|entry| entry.speed_class.as_deref())
            .map(SpeedClass::from_name)
    };
    // Co-located classes never reach the matrix (one id per cell), so the graph
    // reads them from `overlays`: a taxiway crossing a 5 km/h service road pays
    // 5 km/h on that cell instead of the taxi speed the matrix shows.
    let overlay_of = |x: i64, y: i64| document.overlay_at(x, y).map(<[i64]>::to_vec);
    let graph = graph_from_grid_with_overlays(
        &document.map,
        &road_of,
        &walk_of,
        &overlay_of,
        &closed,
        cell_size_m,
        origin_lat,
        origin_lon,
    );

    let zones = if document.zones.is_empty() {
        file.zones
    } else {
        document
            .zones
            .iter()
            .map(|zone| Zone {
                id: zone.id.clone(),
                x: zone.x,
                y: zone.y,
                radius_m: zone.radius_m,
                label: zone.label.clone(),
            })
            .collect()
    };
    let objects = if document.objects.is_empty() {
        file.objects
    } else {
        document
            .objects
            .iter()
            .map(|object| MapObject {
                id: object.id.clone(),
                kind: object.kind.clone(),
                x: object.x + object.w / 2,
                y: object.y + object.h / 2,
            })
            .collect()
    };

    Location {
        cell_size_m,
        sla_secs: file.sla_secs,
        alert_before_secs: file.alert_before_secs,
        boarding_delay_secs: file.boarding_delay_secs,
        tool_delay_secs: file.tool_delay_secs,
        aog_bonus_secs: file.aog_bonus_secs,
        arrival_bonus_secs: file.arrival_bonus_secs,
        arrival_window_secs: file.arrival_window_secs,
        urgency_bonus_secs: file.urgency_bonus_secs,
        mel_bonus_secs: file.mel_bonus_secs,
        max_accuracy_m: file.max_accuracy_m,
        max_speed_mps: document.max_speed_mps(file.max_speed_mps),
        stale_after_secs: file.stale_after_secs,
        zones,
        objects,
        ata,
        agents: file.agents,
        graph,
    }
}

fn default_ata() -> Vec<AtaChapter> {
    vec![
        chapter("21", "Кондиционирование", &["aviation_technician"]),
        chapter(
            "28",
            "Топливная система",
            &["fueling_crew", "engine_technician"],
        ),
        chapter(
            "30",
            "Противообледенительная система",
            &["avionics_engineer", "aviation_technician"],
        ),
        chapter(
            "36",
            "Пневматическая система",
            &["integrity_inspector", "aviation_technician"],
        ),
        chapter("49", "ВСУ", &["engine_technician"]),
        chapter("72", "Двигатель", &["engine_technician"]),
        chapter("34", "Навигация", &["avionics_engineer"]),
        chapter("5", "Замечания экипажа", &["crew_remarks_handler"]),
    ]
}

fn chapter(id: &str, title: &str, admissions: &[&str]) -> AtaChapter {
    AtaChapter {
        chapter: id.to_owned(),
        title: title.to_owned(),
        admissions: admissions.iter().map(|item| (*item).to_owned()).collect(),
    }
}
