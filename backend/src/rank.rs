//! Scores every engineer for one request. The chosen row is the best
//! eligible candidate, not merely the nearest one.

use std::collections::HashMap;
use std::time::Instant;

use uuid::Uuid;

use crate::{
    graph::{AgentKind, TOOL_DELAY_SECS},
    location::Location,
    maps::ActiveMap,
    types::{
        enums::EngineerType,
        map::{Point, Route},
    },
};

#[derive(Debug, Clone)]
pub struct CandidateScore {
    pub engineer_id: Uuid,
    pub eligible: bool,
    pub reason: String,
    pub eta_seconds: u64,
    pub uses_vehicle: bool,
    pub route: Route,
    pub score: f64,
}

pub struct RankOutcome {
    pub candidates: Vec<CandidateScore>,
    pub elapsed_ms: i32,
}

pub struct RankInput<'a> {
    pub map: &'a ActiveMap,
    pub plane: Point,
    pub chapter: Option<&'a str>,
    pub urgency: &'a str,
    pub engineers: Vec<(Uuid, EngineerType)>,
    pub positions: HashMap<Uuid, Point>,
    pub in_progress: HashMap<Uuid, bool>,
    pub load: HashMap<Uuid, u32>,
    pub waiting_same_aircraft: HashMap<Uuid, bool>,
    pub transports: HashMap<Point, Uuid>,
    pub needs_tools: bool,
}

pub fn rank(input: RankInput<'_>) -> RankOutcome {
    let started = Instant::now();
    let mut candidates = Vec::with_capacity(input.engineers.len());
    for (id, kind) in &input.engineers {
        candidates.push(score_one(&input, *id, *kind));
    }
    candidates.sort_by(|left, right| {
        right.eligible.cmp(&left.eligible).then(
            left.score
                .partial_cmp(&right.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    RankOutcome {
        elapsed_ms: started.elapsed().as_millis().min(i32::MAX as u128) as i32,
        candidates,
    }
}

fn score_one(input: &RankInput<'_>, id: Uuid, kind: EngineerType) -> CandidateScore {
    let reject = |reason: &str| CandidateScore {
        engineer_id: id,
        eligible: false,
        reason: reason.to_owned(),
        eta_seconds: 0,
        uses_vehicle: false,
        route: Route::new(Vec::new()),
        score: f64::MAX,
    };
    if let Some(chapter) = input.chapter {
        if !admits(&input.map.location, chapter, kind) {
            return reject("допуск не соответствует главе ATA");
        }
    }
    if input.in_progress.get(&id).copied().unwrap_or(false) {
        return reject("уже ведёт начатую работу");
    }
    if input
        .waiting_same_aircraft
        .get(&id)
        .copied()
        .unwrap_or(false)
    {
        return reject("ожидает запчасть на этом борту");
    }
    let Some(position) = input.positions.get(&id).copied() else {
        return reject("нет позиции на карте");
    };

    let walk = walk_eta(input, position);
    let vehicle = vehicle_eta(input, position);
    let (eta, route, uses_vehicle) = match (walk, vehicle) {
        (Ok((walk_secs, _walk_route)), Ok((vehicle_secs, vehicle_route)))
            if vehicle_secs + input.map.location.boarding_delay_secs < walk_secs =>
        {
            (
                vehicle_secs + input.map.location.boarding_delay_secs,
                vehicle_route,
                true,
            )
        }
        (Ok((walk_secs, walk_route)), _) => (walk_secs, walk_route, false),
        (Err(_), Ok((vehicle_secs, vehicle_route))) => (
            vehicle_secs + input.map.location.boarding_delay_secs,
            vehicle_route,
            true,
        ),
        (Err(error), Err(_)) => return reject(&error),
    };
    let eta_seconds = eta.ceil() as u64;
    let load = input.load.get(&id).copied().unwrap_or(0);
    let sla = input.map.location.sla_secs as f64;
    let late = if eta > sla {
        10_000.0 + (eta - sla)
    } else {
        0.0
    };
    let urgency_bias = if input.urgency == "aog" { 0.0 } else { 0.0 };
    let score = eta + late + f64::from(load) * 20.0 + urgency_bias;
    let reason = if uses_vehicle {
        format!("допуск подходит, свободен, {eta_seconds} с со спецтранспортом, загрузка {load}")
    } else {
        format!("допуск подходит, свободен, {eta_seconds} с пешком, загрузка {load}")
    };
    CandidateScore {
        engineer_id: id,
        eligible: true,
        reason,
        eta_seconds,
        uses_vehicle,
        route: Route::new(route),
        score,
    }
}

fn admits(location: &Location, chapter: &str, kind: EngineerType) -> bool {
    let needle = engineer_type_name(kind);
    location
        .ata
        .iter()
        .find(|item| item.chapter == chapter)
        .map(|item| item.admissions.iter().any(|admission| admission == &needle))
        .unwrap_or(true)
}

pub fn engineer_type_name(kind: EngineerType) -> &'static str {
    match kind {
        EngineerType::IntegrityInspector => "integrity_inspector",
        EngineerType::CrewRemarksHandler => "crew_remarks_handler",
        EngineerType::FuelingCrew => "fueling_crew",
        EngineerType::EngineTechnician => "engine_technician",
        EngineerType::AvionicsEngineer => "avionics_engineer",
        EngineerType::AviationTechnician => "aviation_technician",
    }
}

fn walk_eta(input: &RankInput<'_>, from: Point) -> Result<(f64, Vec<Point>), String> {
    let mut stops = vec![from];
    if input.needs_tools {
        if let Some(warehouse) = input.map.location.object_point("warehouse") {
            stops.push(warehouse);
        }
    }
    stops.push(input.plane);
    let planned = input
        .map
        .location
        .graph
        .route_stops(AgentKind::Engineer, &stops)
        .map_err(|error| error.to_string())?;
    Ok((with_tools(input, planned.seconds), planned.points))
}

fn vehicle_eta(input: &RankInput<'_>, from: Point) -> Result<(f64, Vec<Point>), String> {
    if input.transports.is_empty() {
        return Err("нет спецтранспорта".into());
    }
    let mut best: Option<(f64, Vec<Point>)> = None;
    for vehicle_at in input.transports.keys().copied() {
        let mut approach_stops = vec![from];
        if input.needs_tools {
            if let Some(warehouse) = input.map.location.object_point("warehouse") {
                approach_stops.push(warehouse);
            }
        }
        approach_stops.push(vehicle_at);
        let approach = input
            .map
            .location
            .graph
            .route_stops(AgentKind::Engineer, &approach_stops)
            .map_err(|error| error.to_string())?;
        let haul = input
            .map
            .location
            .graph
            .route_stops(AgentKind::Vehicle, &[vehicle_at, input.plane])
            .map_err(|error| error.to_string())?;
        let seconds = with_tools(input, approach.seconds + haul.seconds);
        let mut points = approach.points;
        if haul.points.len() > 1 {
            points.extend_from_slice(&haul.points[1..]);
        }
        if best
            .as_ref()
            .is_none_or(|(best_secs, _)| seconds < *best_secs)
        {
            best = Some((seconds, points));
        }
    }
    best.ok_or_else(|| "нет спецтранспорта".into())
}

fn with_tools(input: &RankInput<'_>, seconds: f64) -> f64 {
    if input.needs_tools {
        seconds + input.map.location.tool_delay_secs.max(TOOL_DELAY_SECS)
    } else {
        seconds
    }
}
