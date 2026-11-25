//! Scores every engineer for one request. The chosen row is the best
//! eligible candidate, not merely the nearest one.

use std::collections::HashMap;
use std::time::Instant;

use chrono::Utc;
use uuid::Uuid;

use crate::{
    aircraft_context::AircraftContext,
    graph::{AgentKind, TOOL_DELAY_SECS},
    location::Location,
    maps::ActiveMap,
    types::{
        enums::EngineerType,
        map::{Point, Route},
    },
};

/// Every term the ranking adds up, in seconds. [`ScoreComponents::total`]
/// subtracts the two bonuses, so a candidate row explains its own score.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScoreComponents {
    /// Travel time to the aircraft.
    pub eta: f64,
    /// SLA penalty: `10_000 + (eta - sla)` only when the travel time is late.
    pub late: f64,
    /// Workload already multiplied by the per-task weight (seconds of travel).
    pub load: f64,
    /// Post-arrival bonus already faded by the age of the arrival.
    pub arrival_bonus: f64,
    /// AOG bonus actually applied to this candidate.
    pub aog_bonus: f64,
    /// Bonus for an `urgent` request; zero for `normal`.
    pub urgency_bonus: f64,
    /// Bonus for the declared MEL category; zero when the task has none.
    pub mel_bonus: f64,
}

impl ScoreComponents {
    /// `score = eta + late + load - aog_bonus - arrival_bonus - urgency_bonus
    /// - mel_bonus`.
    pub fn total(self) -> f64 {
        self.eta + self.late + self.load
            - self.aog_bonus
            - self.arrival_bonus
            - self.urgency_bonus
            - self.mel_bonus
    }
}

/// One addend of [`CandidateScore::score`], published so a dispatcher can read
/// why a candidate won. `seconds` is signed: a penalty is positive, a bonus
/// negative, and the sum of every factor equals the score.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreFactor {
    /// Stable machine name, for example `eta` or `aog_bonus`.
    pub name: String,
    /// Contribution to the score, in seconds.
    pub seconds: f64,
}

#[derive(Debug, Clone)]
pub struct CandidateScore {
    pub engineer_id: Uuid,
    pub eligible: bool,
    pub reason: String,
    pub eta_seconds: u64,
    pub uses_vehicle: bool,
    pub route: Route,
    /// The terms that produced [`CandidateScore::score`].
    pub components: ScoreComponents,
    /// [`CandidateScore::components`] flattened, in score order. The sum of
    /// every `seconds` equals [`CandidateScore::score`].
    pub factors: Vec<ScoreFactor>,
    /// Sum of [`CandidateScore::components`]; lower is better.
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
    /// Arrival time from `aircraft_movements` and the AOG flag from Redis.
    pub aircraft: AircraftContext,
    pub engineers: Vec<(Uuid, EngineerType)>,
    pub positions: HashMap<Uuid, Point>,
    pub in_progress: HashMap<Uuid, bool>,
    pub load: HashMap<Uuid, u32>,
    /// Request urgency as stored on the task: `normal` or `urgent`.
    pub urgency: &'a str,
    /// MEL category of the defect (`A`..`D`), when the task declares one.
    pub mel_category: Option<&'a str>,
    pub transports: HashMap<Point, Uuid>,
    pub needs_tools: bool,
    /// Engineers forced onto a vehicle regardless of the walking comparison.
    pub forced_vehicle: std::collections::HashSet<Uuid>,
}

/// Key marking an engineer as forced onto a vehicle: `rank:force_vehicle:{id}`.
///
/// The organiser checklist asks for a switch that makes an engineer use transport
/// even when walking would be faster, so the vehicle branch of a candidate can be
/// demonstrated during the trial.
#[must_use]
pub fn forced_vehicle_key(engineer: Uuid) -> String {
    format!("rank:force_vehicle:{engineer}")
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
        components: ScoreComponents::default(),
        factors: Vec::new(),
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
    let Some(position) = input.positions.get(&id).copied() else {
        return reject("нет позиции на карте");
    };

    let walk = walk_eta(input, position);
    let vehicle = vehicle_eta(input, position);
    // A forced engineer takes the vehicle branch whether or not walking is
    // shorter; without a vehicle route the normal comparison still applies.
    let forced = if input.forced_vehicle.contains(&id) {
        vehicle_eta(input, position).ok()
    } else {
        None
    };
    let (eta, route, uses_vehicle) = match forced {
        Some((vehicle_secs, vehicle_route)) => (
            vehicle_secs + input.map.location.boarding_delay_secs,
            vehicle_route,
            true,
        ),
        None => match (walk, vehicle) {
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
        },
    };
    // The published row is rounded up, so the score must be built from that
    // same value: otherwise `score` never equals the sum of its components.
    let eta = eta.ceil();
    let eta_seconds = eta as u64;
    let load = input.load.get(&id).copied().unwrap_or(0);
    let sla = input.map.location.sla_secs as f64;
    let late = if eta > sla {
        10_000.0 + (eta - sla)
    } else {
        0.0
    };
    let components = ScoreComponents {
        eta,
        late,
        load: f64::from(load) * 20.0,
        aog_bonus: aog_bonus(input),
        arrival_bonus: arrival_bonus(input),
        urgency_bonus: urgency_bonus(input),
        mel_bonus: mel_bonus(input),
    };
    let score = components.total();
    let factors = factors(components);
    let reason = reason_text(input, components, eta_seconds, load, uses_vehicle);
    CandidateScore {
        engineer_id: id,
        eligible: true,
        reason,
        eta_seconds,
        uses_vehicle,
        route: Route::new(route),
        components,
        factors,
        score,
    }
}

/// [`ScoreComponents`] as signed addends whose sum is exactly the score. The
/// order is the order the score reads in: travel, then penalties, then bonuses.
fn factors(components: ScoreComponents) -> Vec<ScoreFactor> {
    vec![
        ScoreFactor {
            name: "eta".to_owned(),
            seconds: components.eta,
        },
        ScoreFactor {
            name: "late".to_owned(),
            seconds: components.late,
        },
        ScoreFactor {
            name: "load".to_owned(),
            seconds: components.load,
        },
        ScoreFactor {
            name: "aog_bonus".to_owned(),
            seconds: -components.aog_bonus,
        },
        ScoreFactor {
            name: "arrival_bonus".to_owned(),
            seconds: -components.arrival_bonus,
        },
        ScoreFactor {
            name: "urgency_bonus".to_owned(),
            seconds: -components.urgency_bonus,
        },
        ScoreFactor {
            name: "mel_bonus".to_owned(),
            seconds: -components.mel_bonus,
        },
    ]
}

/// Every term that moved the score, in Russian, so an accepted row explains
/// itself: admission, travel, workload, and each bonus that fired.
fn reason_text(
    input: &RankInput<'_>,
    components: ScoreComponents,
    eta_seconds: u64,
    load: u32,
    uses_vehicle: bool,
) -> String {
    let travel = if uses_vehicle {
        format!("{eta_seconds} с со спецтранспортом")
    } else {
        format!("{eta_seconds} с пешком")
    };
    let mut reason = format!("допуск подходит, свободен, {travel}, загрузка {load}");
    if components.late > 0.0 {
        reason.push_str(&format!(", штраф за опоздание {:.0} с", components.late));
    }
    if input.needs_tools {
        reason.push_str(", нужен склад");
    }
    if components.aog_bonus > 0.0 {
        reason.push_str(&format!(", бонус AOG {:.0} с", components.aog_bonus));
    }
    if components.arrival_bonus > 0.0 {
        reason.push_str(&format!(
            ", бонус за прилёт {:.0} с",
            components.arrival_bonus
        ));
    }
    if components.urgency_bonus > 0.0 {
        reason.push_str(&format!(
            ", бонус срочности {:.0} с",
            components.urgency_bonus
        ));
    }
    if let Some(category) = input.mel_category {
        if components.mel_bonus > 0.0 {
            reason.push_str(&format!(
                ", MEL {category}: бонус {:.0} с",
                components.mel_bonus
            ));
        }
    }
    reason
}

/// Bonus for an `urgent` request. Anything else (`normal`) scores zero, so the
/// urgency the dispatcher typed actually reaches the ranking.
fn urgency_bonus(input: &RankInput<'_>) -> f64 {
    if input.urgency.eq_ignore_ascii_case("urgent") {
        input.map.location.urgency_bonus_secs
    } else {
        0.0
    }
}

/// Bonus for the declared MEL category, scaled from `mel_bonus_secs` (the
/// weight of category `B`): `A` presses hardest, `C` and `D` less. With the
/// default base the scale is 120/90/60/30 seconds. An unknown or absent
/// category carries nothing.
fn mel_bonus(input: &RankInput<'_>) -> f64 {
    let base = input.map.location.mel_bonus_secs;
    let factor = match input.mel_category.map(str::to_ascii_uppercase).as_deref() {
        Some("A") => 4.0 / 3.0,
        Some("B") => 1.0,
        Some("C") => 2.0 / 3.0,
        Some("D") => 1.0 / 3.0,
        _ => return 0.0,
    };
    base * factor
}

/// AOG bonus for a grounded aircraft. `needs_tools` (a warehouse stop for
/// tools or parts) carries the full weight, a request that needs no stop half
/// of it — `needs_tools` is the closest the code gets to "parts required".
fn aog_bonus(input: &RankInput<'_>) -> f64 {
    if !input.aircraft.aog {
        return 0.0;
    }
    let factor = if input.needs_tools { 1.0 } else { 0.5 };
    input.map.location.aog_bonus_secs * factor
}

/// Bonus for an aircraft that has just arrived: full weight at touchdown,
/// fading linearly to zero across the configured window. A departed aircraft
/// carries no `arrived_at` at all (the loader drops it), an unknown one gets 0.
fn arrival_bonus(input: &RankInput<'_>) -> f64 {
    let window = input.map.location.arrival_window_secs as f64;
    let Some(arrived_at) = input.aircraft.arrived_at else {
        return 0.0;
    };
    if window <= 0.0 {
        return 0.0;
    }
    let age = (Utc::now() - arrived_at).num_seconds().max(0) as f64;
    if age >= window {
        return 0.0;
    }
    input.map.location.arrival_bonus_secs * (1.0 - age / window)
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
