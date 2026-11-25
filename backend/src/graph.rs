//! Road graph with per-edge speed and agent access.
//!
//! A cell map becomes a graph of walkable cells. A point that is not on a
//! node is snapped to the nearest open edge the agent is allowed to use.
//! Every route is a time-weighted A* search on this graph.
//!
//! # From GPS degrees to a cell of the graph
//!
//! A tracker reports WGS-84 degrees, while the map, the graph and every route
//! are cells. The two are related by an equirectangular projection anchored at
//! the north-west corner of the map ([`RoadGraph::origin_lat`],
//! [`RoadGraph::origin_lon`]), with one cell per [`RoadGraph::cell_size_m`]
//! metres:
//!
//! ```text
//! y = (origin_lat - lat) * 111320 / cell_size_m
//! x = (lon - origin_lon) * 111320 * cos(origin_lat) / cell_size_m
//! ```
//!
//! `y` grows south so it matches `map[y][x]` row order, and the longitude term
//! is stretched by `cos(origin_lat)` so a cell stays square in metres at the
//! latitude of the airport. 111 320 m is one degree of latitude. The factors
//! are the ones an airport-scale map can use: over a few kilometres the error
//! against a proper geodesic is far below a cell.
//!
//! Ingest runs that projection and then snaps the fractional cell onto the
//! road layer: [`RoadGraph::unproject`] gives `(x, y)` as `f64`, and
//! [`RoadGraph::snap`] picks the nearest open edge the agent may use and
//! answers one of its end nodes, so a GPS fix never lands in a building or on a
//! cell no route can reach. [`RoadGraph::nearest_node`] does the same for a cell
//! that was named directly — it answers the cell's own node when there is one,
//! otherwise the nearest node by Manhattan distance.
//!
//! GNSS error is what this cannot fix: a phone drifting between two taxiways, or
//! an antenna offset, snaps to the same wrong cell every time. That is what the
//! manual pin is for — `crate::calibration` stores "this object is on this cell"
//! and the ingest uses it instead of the snapped one
//! ([`RoadGraph::is_node`] is what a pin is checked against, so a pin always
//! names a cell a route can start or end on). [`RoadGraph::project`] is the
//! inverse, used to show a pinned cell back in degrees.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::types::map::Point;

pub const WALK_KMH: f64 = 5.0;
pub const VEHICLE_STAND_KMH: f64 = 5.0;
pub const VEHICLE_TERMINAL_KMH: f64 = 8.0;
pub const VEHICLE_APRON_KMH: f64 = 10.0;
pub const VEHICLE_AIRFIELD_KMH: f64 = 20.0;
/// Runway section. The real map posts 300 km/h there, which no vehicle in the
/// palette can exceed; the class exists so the section keeps its own speed
/// instead of collapsing into apron speed.
pub const RUNWAY_KMH: f64 = 300.0;
pub const TOOL_DELAY_SECS: f64 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Engineer,
    Vehicle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SpeedClass {
    Walk,
    VehicleStand,
    VehicleTerminal,
    VehicleApron,
    VehicleAirfield,
    Runway,
}

impl SpeedClass {
    /// Name used by the map palette (`palette[].speed_class`). The inverse of
    /// [`SpeedClass::from_name`]; kept public so a writer of map files (or a
    /// test) can spell a class without duplicating the table.
    #[must_use]
    #[allow(dead_code)]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Walk => "Walk",
            Self::VehicleStand => "VehicleStand",
            Self::VehicleTerminal => "VehicleTerminal",
            Self::VehicleApron => "VehicleApron",
            Self::VehicleAirfield => "VehicleAirfield",
            Self::Runway => "Runway",
        }
    }

    /// Palette name to class. An unknown name is an apron class: the map file
    /// may grow kinds the backend does not know yet, and refusing the whole
    /// map over one label would be worse than driving slowly.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "Walk" => Self::Walk,
            "VehicleStand" => Self::VehicleStand,
            "VehicleTerminal" => Self::VehicleTerminal,
            "VehicleAirfield" => Self::VehicleAirfield,
            "Runway" => Self::Runway,
            _ => Self::VehicleApron,
        }
    }

    /// Posted speed of this class, km/h.
    #[must_use]
    pub fn kmh(self) -> f64 {
        match self {
            Self::Walk => walk_speed_kmh(),
            Self::VehicleStand => vehicle_stand_speed_kmh(),
            Self::VehicleTerminal => vehicle_terminal_speed_kmh(),
            Self::VehicleApron => vehicle_apron_speed_kmh(),
            Self::VehicleAirfield => vehicle_airfield_speed_kmh(),
            Self::Runway => runway_speed_kmh(),
        }
    }
}

/// Runway speed. Later this can depend on the cell.
pub fn runway_speed_kmh() -> f64 {
    RUNWAY_KMH
}

/// Walking speed. Later this can depend on the cell; today it is the constant.
pub fn walk_speed_kmh() -> f64 {
    WALK_KMH
}

/// Vehicle speed on a stand. Later this can depend on the cell.
pub fn vehicle_stand_speed_kmh() -> f64 {
    VEHICLE_STAND_KMH
}

/// Vehicle speed in a terminal. Later this can depend on the cell.
pub fn vehicle_terminal_speed_kmh() -> f64 {
    VEHICLE_TERMINAL_KMH
}

/// Vehicle speed on the apron. Later this can depend on the cell.
pub fn vehicle_apron_speed_kmh() -> f64 {
    VEHICLE_APRON_KMH
}

/// Vehicle speed on the airfield. Later this can depend on the cell.
pub fn vehicle_airfield_speed_kmh() -> f64 {
    VEHICLE_AIRFIELD_KMH
}

fn kmh_to_mps(kmh: f64) -> f64 {
    kmh * 1000.0 / 3600.0
}

/// Engineer speed at a cell, metres per second.
pub fn engineer_speed_mps(_at: Point) -> f64 {
    kmh_to_mps(walk_speed_kmh())
}

/// Speed a ground vehicle may actually travel on a class, km/h.
///
/// The posted speed of a runway section (300 km/h) is the aircraft's, not a
/// tug's: a vehicle entering it keeps the airfield cap. The posted speed stays
/// in the palette, where the GPS plausibility filter reads it.
#[must_use]
pub fn vehicle_cap_kmh(class: SpeedClass) -> f64 {
    match class {
        SpeedClass::Runway => vehicle_airfield_speed_kmh(),
        other => other.kmh(),
    }
}

/// Vehicle speed at a cell for an edge class, metres per second.
pub fn vehicle_speed_mps(_at: Point, class: SpeedClass) -> f64 {
    kmh_to_mps(vehicle_cap_kmh(class))
}

/// Upper bound used by A*. Future per-cell speeds must stay at or below it.
pub fn max_engineer_speed_mps() -> f64 {
    engineer_speed_mps(Point::new(0, 0))
}

/// Upper bound used by A*. Future per-cell speeds must stay at or below it.
pub fn max_vehicle_speed_mps() -> f64 {
    let origin = Point::new(0, 0);
    [
        SpeedClass::Walk,
        SpeedClass::VehicleStand,
        SpeedClass::VehicleTerminal,
        SpeedClass::VehicleApron,
        SpeedClass::VehicleAirfield,
        SpeedClass::Runway,
    ]
    .into_iter()
    .map(|class| vehicle_speed_mps(origin, class))
    .fold(0.0, f64::max)
}

fn travel_speed_mps(agent: AgentKind, class: SpeedClass, at: Point) -> f64 {
    match agent {
        AgentKind::Engineer => engineer_speed_mps(at),
        AgentKind::Vehicle => vehicle_speed_mps(at, class),
    }
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone)]
struct GraphEdge {
    to: u32,
    length_m: f64,
    speed: SpeedClass,
    engineer: bool,
    vehicle: bool,
    closed: bool,
}

#[derive(Debug, Clone)]
pub struct RoadGraph {
    nodes: Vec<GraphNode>,
    index: HashMap<(i64, i64), u32>,
    adj: Vec<Vec<GraphEdge>>,
    pub cell_size_m: f64,
    pub origin_lat: f64,
    pub origin_lon: f64,
}

/// One leg of a planned route: a straight line between two cells together with
/// the speed the agent may travel it at and the time it costs.
///
/// `route_leg` builds these from the very edges A* walked, so a caller that
/// needs the speed profile of a route (a simulation, a per-segment ETA) does not
/// have to look the edges up again and cannot disagree with the search that
/// chose them.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RouteSegment {
    pub from: Point,
    pub to: Point,
    /// Length of the straight line between `from` and `to`, in metres.
    pub length_m: f64,
    /// Speed the agent travels this segment at, in metres per second.
    pub speed_mps: f64,
    /// `length_m / speed_mps`.
    pub seconds: f64,
    /// Seconds from the start of the leg to the end of this segment, so the last
    /// segment carries the leg's total time.
    pub cum_seconds: f64,
    /// Road class of the segment. The off-graph approach and departure pieces are
    /// apron pieces, exactly as they are costed.
    pub class: SpeedClass,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlannedRoute {
    pub points: Vec<Point>,
    pub seconds: f64,
    /// The same route as timed, speed-classed segments. Empty when the route has
    /// fewer than two points, i.e. when the object does not move.
    pub segments: Vec<RouteSegment>,
}

impl RoadGraph {
    #[allow(dead_code)]
    /// Cell -> WGS-84. `y` grows south, matching `map[y][x]` row order, so
    /// row 0 is the northern edge of the map.
    pub fn project(&self, x: i64, y: i64) -> (f64, f64) {
        self.project_f64(x as f64, y as f64)
    }

    /// Cell -> WGS-84 for a fractional cell.
    ///
    /// An object between two cells (a simulation sample, a smoothed track point)
    /// has an `(x, y)` that is not an integer; this is what turns it into the
    /// degrees the rest of the app publishes, without rounding it back onto a
    /// cell and losing the movement.
    pub fn project_f64(&self, x: f64, y: f64) -> (f64, f64) {
        let south = y * self.cell_size_m / 111_320.0;
        let lat = self.origin_lat - south;
        let cos = (self.origin_lat.to_radians()).cos().max(0.2);
        let east = x * self.cell_size_m / (111_320.0 * cos);
        (lat, self.origin_lon + east)
    }

    /// WGS-84 -> cell, inverse of [`RoadGraph::project`].
    pub fn unproject(&self, lat: f64, lon: f64) -> (f64, f64) {
        let y = (self.origin_lat - lat) * 111_320.0 / self.cell_size_m;
        let cos = (self.origin_lat.to_radians()).cos().max(0.2);
        let x = (lon - self.origin_lon) * 111_320.0 * cos / self.cell_size_m;
        (x, y)
    }

    /// Whether the cell itself carries a graph node: a route may start or end
    /// exactly here, with no snapping to a neighbouring cell.
    pub fn is_node(&self, point: Point) -> bool {
        self.index.contains_key(&(point.0, point.1))
    }

    pub fn nearest_node(&self, point: Point) -> Option<u32> {
        if let Some(id) = self.index.get(&(point.0, point.1)) {
            return Some(*id);
        }
        self.nodes
            .iter()
            .enumerate()
            .min_by_key(|(_, node)| (node.x - point.0).abs() + (node.y - point.1).abs())
            .map(|(id, _)| id as u32)
    }

    /// Snaps a WGS-84 point to the nearest open edge and returns the grid node
    /// plus the off-graph approach distance in metres.
    pub fn snap(&self, lat: f64, lon: f64, agent: AgentKind) -> Option<(Point, f64)> {
        let (x, y) = self.unproject(lat, lon);
        let mut best: Option<(f64, u32)> = None;
        for (from, edges) in self.adj.iter().enumerate() {
            let a = &self.nodes[from];
            for edge in edges {
                if edge.closed || !edge.allows(agent) {
                    continue;
                }
                let b = &self.nodes[edge.to as usize];
                let dist =
                    distance_to_segment(x, y, a.x as f64, a.y as f64, b.x as f64, b.y as f64);
                let end = if dist_sq(x, y, a.x as f64, a.y as f64)
                    <= dist_sq(x, y, b.x as f64, b.y as f64)
                {
                    from as u32
                } else {
                    edge.to
                };
                if best.is_none_or(|(best_dist, _)| dist < best_dist) {
                    best = Some((dist, end));
                }
            }
        }
        let (cells, node) = best?;
        let point = {
            let node = &self.nodes[node as usize];
            Point::new(node.x, node.y)
        };
        Some((point, cells * self.cell_size_m))
    }

    pub fn route_stops(&self, agent: AgentKind, stops: &[Point]) -> anyhow::Result<PlannedRoute> {
        if stops.len() < 2 {
            return Ok(PlannedRoute {
                points: stops.to_vec(),
                seconds: 0.0,
                segments: Vec::new(),
            });
        }
        let mut points = Vec::new();
        let mut segments: Vec<RouteSegment> = Vec::new();
        for pair in stops.windows(2) {
            let leg = self.route_leg(agent, pair[0], pair[1])?;
            if points.is_empty() {
                points.extend(leg.points);
            } else if leg.points.len() > 1 {
                points.extend_from_slice(&leg.points[1..]);
            }
            // A leg's cumulative times are relative to its own start, so the
            // second and later legs are shifted onto the route's clock.
            let offset = segments.last().map_or(0.0, |segment| segment.cum_seconds);
            for mut segment in leg.segments {
                segment.cum_seconds += offset;
                segments.push(segment);
            }
        }
        let seconds = segments.last().map_or(0.0, |segment| segment.cum_seconds);
        Ok(PlannedRoute {
            points,
            seconds,
            segments,
        })
    }

    fn route_leg(
        &self,
        agent: AgentKind,
        start: Point,
        end: Point,
    ) -> anyhow::Result<PlannedRoute> {
        let Some(start_id) = self.nearest_node(start) else {
            anyhow::bail!("road graph is empty");
        };
        let Some(end_id) = self.nearest_node(end) else {
            anyhow::bail!("road graph is empty");
        };
        let start_node = &self.nodes[start_id as usize];
        let end_node = &self.nodes[end_id as usize];
        let approach = self.cell_size_m
            * (((start.0 - start_node.x).abs() + (start.1 - start_node.y).abs()) as f64);
        let departure =
            self.cell_size_m * (((end.0 - end_node.x).abs() + (end.1 - end_node.y).abs()) as f64);
        let approach_speed = travel_speed_mps(agent, SpeedClass::VehicleApron, start);
        let departure_speed = travel_speed_mps(agent, SpeedClass::VehicleApron, end);
        let start_point = Point::new(start_node.x, start_node.y);
        let end_point = Point::new(end_node.x, end_node.y);

        if start_id == end_id {
            let mut points = vec![start, start_point];
            if end != points[points.len() - 1] {
                points.push(end);
            }
            let mut segments = Vec::new();
            push_segment(
                &mut segments,
                start,
                start_point,
                approach,
                approach_speed,
                SpeedClass::VehicleApron,
            );
            push_segment(
                &mut segments,
                end_point,
                end,
                departure,
                departure_speed,
                SpeedClass::VehicleApron,
            );
            let seconds = segments.last().map_or(0.0, |segment| segment.cum_seconds);
            return Ok(PlannedRoute {
                points,
                seconds,
                segments,
            });
        }

        let (path, _travel) = self.astar(agent, start_id, end_id)?;
        let mut points = Vec::with_capacity(path.len() + 2);
        if start != start_point {
            points.push(start);
        }
        for id in &path {
            let node = &self.nodes[*id as usize];
            points.push(Point::new(node.x, node.y));
        }
        if end != *points.last().unwrap_or(&end) {
            points.push(end);
        }

        // The approach is a straight off-graph line, the middle is the edges A*
        // actually took, and the departure closes the leg from the last node to
        // the requested cell. Rebuilding the segment times from those same edges
        // keeps the sum equal to the leg's total.
        let mut segments = Vec::new();
        push_segment(
            &mut segments,
            start,
            start_point,
            approach,
            approach_speed,
            SpeedClass::VehicleApron,
        );
        for pair in path.windows(2) {
            let from = Point::new(
                self.nodes[pair[0] as usize].x,
                self.nodes[pair[0] as usize].y,
            );
            let to = Point::new(
                self.nodes[pair[1] as usize].x,
                self.nodes[pair[1] as usize].y,
            );
            let Some(edge) = self.edge_between(agent, pair[0], pair[1]) else {
                continue;
            };
            let speed = travel_speed_mps(agent, edge.speed, from);
            push_segment(&mut segments, from, to, edge.length_m, speed, edge.speed);
        }
        push_segment(
            &mut segments,
            end_point,
            end,
            departure,
            departure_speed,
            SpeedClass::VehicleApron,
        );
        let seconds = segments.last().map_or(0.0, |segment| segment.cum_seconds);
        Ok(PlannedRoute {
            points,
            seconds,
            segments,
        })
    }

    /// The edge A* walked from `from` to `to`: the fastest of the parallel edges,
    /// which is the one the search relaxed. `None` when the two nodes are not
    /// joined by an edge this agent may use, so the segment is left out instead of
    /// inventing a speed for it.
    fn edge_between(&self, agent: AgentKind, from: u32, to: u32) -> Option<&GraphEdge> {
        let at = {
            let node = &self.nodes[from as usize];
            Point::new(node.x, node.y)
        };
        self.adj[from as usize]
            .iter()
            .filter(|edge| edge.to == to && !edge.closed && edge.allows(agent))
            .min_by(|left, right| {
                let left_secs =
                    left.length_m / travel_speed_mps(agent, left.speed, at).max(f64::MIN_POSITIVE);
                let right_secs = right.length_m
                    / travel_speed_mps(agent, right.speed, at).max(f64::MIN_POSITIVE);
                left_secs.total_cmp(&right_secs)
            })
    }

    /// Time-optimal path. The heuristic is straight-line distance at the
    /// agent's maximum speed, so the first time the goal is settled it is optimal,
    /// and a long route does not scan the whole component.
    fn astar(&self, agent: AgentKind, start: u32, end: u32) -> anyhow::Result<(Vec<u32>, f64)> {
        let node_count = self.nodes.len();
        let mut best = vec![f64::INFINITY; node_count];
        let mut prev = vec![u32::MAX; node_count];
        let mut settled = vec![false; node_count];
        let max_speed = match agent {
            AgentKind::Engineer => max_engineer_speed_mps(),
            AgentKind::Vehicle => max_vehicle_speed_mps(),
        }
        .max(f64::MIN_POSITIVE);
        let heuristic = |id: u32| {
            let from = &self.nodes[id as usize];
            let goal = &self.nodes[end as usize];
            let dx = (from.x - goal.x) as f64 * self.cell_size_m;
            let dy = (from.y - goal.y) as f64 * self.cell_size_m;
            (dx * dx + dy * dy).sqrt() / max_speed
        };

        best[start as usize] = 0.0;
        let mut heap = BinaryHeap::new();
        heap.push(State {
            score: heuristic(start),
            seconds: 0.0,
            node: start,
        });
        while let Some(State { seconds, node, .. }) = heap.pop() {
            if settled[node as usize] || seconds > best[node as usize] {
                continue;
            }
            settled[node as usize] = true;
            if node == end {
                break;
            }
            let at = {
                let node = &self.nodes[node as usize];
                Point::new(node.x, node.y)
            };
            for edge in &self.adj[node as usize] {
                if edge.closed || !edge.allows(agent) || settled[edge.to as usize] {
                    continue;
                }
                let speed = travel_speed_mps(agent, edge.speed, at).max(f64::MIN_POSITIVE);
                let next = seconds + edge.length_m / speed;
                let to = edge.to as usize;
                if next < best[to] {
                    best[to] = next;
                    prev[to] = node;
                    heap.push(State {
                        score: next + heuristic(edge.to),
                        seconds: next,
                        node: edge.to,
                    });
                }
            }
        }
        if !best[end as usize].is_finite() {
            anyhow::bail!("no route on the open road graph");
        }
        let mut path = Vec::new();
        let mut cursor = end;
        loop {
            path.push(cursor);
            if cursor == start {
                break;
            }
            let parent = prev[cursor as usize];
            if parent == u32::MAX {
                anyhow::bail!("no route on the open road graph");
            }
            cursor = parent;
        }
        path.reverse();
        Ok((path, best[end as usize]))
    }
}

/// Appends one timed segment, skipping the empty ones a route produces when a
/// stop already sits on its graph node.
fn push_segment(
    segments: &mut Vec<RouteSegment>,
    from: Point,
    to: Point,
    length_m: f64,
    speed_mps: f64,
    class: SpeedClass,
) {
    if from == to || length_m <= f64::EPSILON || speed_mps <= f64::MIN_POSITIVE {
        return;
    }
    let seconds = length_m / speed_mps;
    let cum_seconds = segments.last().map_or(0.0, |last| last.cum_seconds) + seconds;
    segments.push(RouteSegment {
        from,
        to,
        length_m,
        speed_mps,
        seconds,
        cum_seconds,
        class,
    });
}

impl GraphEdge {
    fn allows(&self, agent: AgentKind) -> bool {
        match agent {
            AgentKind::Engineer => self.engineer,
            AgentKind::Vehicle => self.vehicle,
        }
    }
}

#[derive(Copy, Clone)]
struct State {
    score: f64,
    seconds: f64,
    node: u32,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score && self.seconds == other.seconds && self.node == other.node
    }
}
impl Eq for State {}
impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; the smallest f-score must come out first.
        other
            .score
            .total_cmp(&self.score)
            .then_with(|| other.seconds.total_cmp(&self.seconds))
            .then_with(|| other.node.cmp(&self.node))
    }
}

fn dist_sq(x: f64, y: f64, ax: f64, ay: f64) -> f64 {
    let dx = x - ax;
    let dy = y - ay;
    dx * dx + dy * dy
}

fn distance_to_segment(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return dist_sq(px, py, ax, ay).sqrt();
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0);
    dist_sq(px, py, ax + t * dx, ay + t * dy).sqrt()
}

pub struct GraphBuilder {
    nodes: Vec<GraphNode>,
    index: HashMap<(i64, i64), u32>,
    adj: Vec<Vec<GraphEdge>>,
    cell_size_m: f64,
    origin_lat: f64,
    origin_lon: f64,
}

impl GraphBuilder {
    pub fn new(cell_size_m: f64, origin_lat: f64, origin_lon: f64) -> Self {
        Self {
            nodes: Vec::new(),
            index: HashMap::new(),
            adj: Vec::new(),
            cell_size_m,
            origin_lat,
            origin_lon,
        }
    }

    pub fn add_node(&mut self, x: i64, y: i64) -> u32 {
        if let Some(id) = self.index.get(&(x, y)) {
            return *id;
        }
        let id = self.nodes.len() as u32;
        self.nodes.push(GraphNode { x, y });
        self.index.insert((x, y), id);
        self.adj.push(Vec::new());
        id
    }

    pub fn add_edge(
        &mut self,
        from: u32,
        to: u32,
        length_m: f64,
        speed: SpeedClass,
        engineer: bool,
        vehicle: bool,
        closed: bool,
    ) {
        self.adj[from as usize].push(GraphEdge {
            to,
            length_m,
            speed,
            engineer,
            vehicle,
            closed,
        });
    }

    pub fn finish(self) -> RoadGraph {
        RoadGraph {
            nodes: self.nodes,
            index: self.index,
            adj: self.adj,
            cell_size_m: self.cell_size_m,
            origin_lat: self.origin_lat,
            origin_lon: self.origin_lon,
        }
    }
}

/// Cells a palette marks as traversable become nodes; four-neighbour links
/// become edges.
///
/// Each edge carries the [`SpeedClass`] of the cell being entered, so a route
/// costs what the map posts per section instead of one flat apron speed.
/// `road_of` returns the drivable class of a cell (and `None` when a vehicle
/// may not enter it), `walk_of` the walkable one. A cell that is walkable but
/// not drivable — a pedestrian path — gets an engineer-only edge, which is how
/// the engineer leaves the apron in the real airport map.
#[cfg(test)]
pub fn graph_from_grid(
    matrix: &crate::types::map::MapMatrix,
    road_of: &dyn Fn(i64) -> Option<SpeedClass>,
    walk_of: &dyn Fn(i64) -> Option<SpeedClass>,
    closed: &HashSet<(i64, i64)>,
    cell_size_m: f64,
    origin_lat: f64,
    origin_lon: f64,
) -> RoadGraph {
    graph_from_grid_with_overlays(
        matrix,
        road_of,
        walk_of,
        &|_, _| None,
        closed,
        cell_size_m,
        origin_lat,
        origin_lon,
    )
}

/// [`graph_from_grid`], plus the classes co-located on a cell.
///
/// A v2 map stores one palette id per cell, so where a taxiway crosses a
/// 20 km/h service road only one speed reaches the matrix. `overlay_of` returns
/// the other ids for a cell; the edge then costs the **slowest** drivable class
/// there, because a vehicle on that cell may drive under the strictest limit it
/// is subject to. Without overlays this is exactly [`graph_from_grid`].
pub fn graph_from_grid_with_overlays(
    matrix: &crate::types::map::MapMatrix,
    road_of: &dyn Fn(i64) -> Option<SpeedClass>,
    walk_of: &dyn Fn(i64) -> Option<SpeedClass>,
    overlay_of: &dyn Fn(i64, i64) -> Option<Vec<i64>>,
    closed: &HashSet<(i64, i64)>,
    cell_size_m: f64,
    origin_lat: f64,
    origin_lon: f64,
) -> RoadGraph {
    let mut builder = GraphBuilder::new(cell_size_m, origin_lat, origin_lon);
    for (y, row) in matrix.0.iter().enumerate() {
        for (x, value) in row.iter().enumerate() {
            if road_of(*value).is_some() || walk_of(*value).is_some() {
                builder.add_node(x as i64, y as i64);
            }
        }
    }
    let coords: Vec<(i64, i64)> = builder.index.keys().copied().collect();
    for (x, y) in coords {
        let from = builder.index[&(x, y)];
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            let Some(&to) = builder.index.get(&(nx, ny)) else {
                continue;
            };
            let value = matrix.0[ny as usize][nx as usize];
            let source_value = matrix.0[y as usize][x as usize];
            let source_road = road_of(source_value);
            let target_road = road_of(value);
            let source_walk = walk_of(source_value);
            let target_walk = walk_of(value);
            let vehicle = source_road.is_some() && target_road.is_some();
            let engineer = source_walk.is_some() && target_walk.is_some();
            if !vehicle && !engineer {
                continue;
            }
            // A section is a corridor: you keep its speed while both cells
            // belong to it, and slow to the lower of the two when crossing
            // into a slower class. That is why a route pays the posted speed of
            // the section it runs along, not one flat apron speed.
            let speed = match (source_road, target_road) {
                (Some(a), Some(b)) => {
                    let mut slowest = if a.kmh() <= b.kmh() { a } else { b };
                    // A co-located class on either cell tightens the limit.
                    for (cx, cy) in [(x, y), (nx, ny)] {
                        let Some(ids) = overlay_of(cx, cy) else {
                            continue;
                        };
                        for id in ids {
                            if let Some(class) = road_of(id) {
                                if class.kmh() < slowest.kmh() {
                                    slowest = class;
                                }
                            }
                        }
                    }
                    slowest
                }
                _ => source_walk.or(target_walk).unwrap_or(SpeedClass::Walk),
            };
            builder.add_edge(
                from,
                to,
                cell_size_m,
                speed,
                engineer,
                vehicle,
                closed.contains(&(x, y)) || closed.contains(&(nx, ny)),
            );
        }
    }
    builder.finish()
}

/// Convenience wrapper for the legacy shape: one set of values that is both
/// drivable and walkable at the same speed, which is what a v1 map means.
///
/// The loader never calls it — it goes through [`graph_from_grid`] with the
/// palette, and a v1 document synthesises apron-speed entries there. This
/// exists for tests that want a flat grid in one line.
#[cfg(test)]
pub fn graph_from_legacy_grid(
    matrix: &crate::types::map::MapMatrix,
    road_values: &HashSet<i64>,
    closed: &HashSet<(i64, i64)>,
    cell_size_m: f64,
    origin_lat: f64,
    origin_lon: f64,
) -> RoadGraph {
    let road_of = |value: i64| road_values.contains(&value).then_some(SpeedClass::VehicleApron);
    graph_from_grid(
        matrix,
        &road_of,
        &road_of,
        closed,
        cell_size_m,
        origin_lat,
        origin_lon,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::map::MapMatrix;
    use std::collections::HashSet;

    #[test]
    fn route_uses_walk_speed_and_snaps_off_graph() {
        let matrix = MapMatrix(vec![vec![1, 1, 1], vec![0, 0, 0], vec![1, 1, 1]]);
        let roads = HashSet::from([1]);
        let graph = graph_from_legacy_grid(&matrix, &roads, &HashSet::new(), 10.0, 55.83, 37.63);
        let route = graph
            .route_stops(AgentKind::Engineer, &[Point::new(0, 0), Point::new(2, 0)])
            .expect("route");
        assert!(route.points.len() >= 2);
        let metres = 20.0;
        let expected = metres / engineer_speed_mps(Point::new(0, 0));
        assert!((route.seconds - expected).abs() < 0.2, "{}", route.seconds);

        let vehicle = graph
            .route_stops(AgentKind::Vehicle, &[Point::new(0, 0), Point::new(2, 0)])
            .expect("vehicle route");
        let vehicle_expected =
            metres / vehicle_speed_mps(Point::new(0, 0), SpeedClass::VehicleApron);
        assert!(
            (vehicle.seconds - vehicle_expected).abs() < 0.2,
            "{}",
            vehicle.seconds
        );
        assert!(vehicle.seconds < route.seconds);

        let (lat, lon) = graph.project(1, 1);
        let (snapped, approach) = graph.snap(lat, lon, AgentKind::Engineer).expect("snap");
        assert!(snapped.1 == 0 || snapped.1 == 2, "{snapped:?}");
        assert!(approach > 0.0);
    }

    #[test]
    fn a_fast_section_actually_shortens_vehicle_travel_time() {
        // Two lanes between the same two columns. The upper lane is apron in
        // both graphs; the lower lane is apron in the flat graph and a runway
        // section in the palette graph. Speed is paid per section, so the
        // paletted route runs the lower lane and arrives sooner.
        let matrix = MapMatrix(vec![
            vec![9, 9, 9, 9, 9],
            vec![9, 9, 9, 9, 9],
            vec![1, 1, 1, 1, 1],
            vec![9, 9, 9, 9, 9],
        ]);
        let flat = graph_from_legacy_grid(
            &matrix,
            &HashSet::from([9, 1]),
            &HashSet::new(),
            5.0,
            55.83,
            37.63,
        );
        let palette = graph_from_grid(
            &matrix,
            &|value| match value {
                0 => None,
                1 => Some(SpeedClass::Runway),
                _ => Some(SpeedClass::VehicleApron),
            },
            &|value| (value != 0).then_some(SpeedClass::VehicleApron),
            &HashSet::new(),
            5.0,
            55.83,
            37.63,
        );
        let stops = [Point::new(0, 2), Point::new(4, 2)];
        let flat_route = flat.route_stops(AgentKind::Vehicle, &stops).expect("flat route");
        let palette_route = palette
            .route_stops(AgentKind::Vehicle, &stops)
            .expect("palette route");
        assert!(
            palette_route.seconds < flat_route.seconds * 0.75,
            "a fast section must beat the flat apron: {} vs {}",
            palette_route.seconds,
            flat_route.seconds
        );
        // A vehicle keeps the airfield cap on a runway section, not 300 km/h.
        assert!(palette_route.seconds > 0.5);
    }

    #[test]
    fn an_overlapped_cell_pays_the_slower_colocated_speed() {
        // The matrix shows 20 km/h service cells (id 2) all the way. Cells
        // (1,0) and (2,0) are *also* a 5 km/h service road (id 3), which the
        // single-value matrix cannot show. The route must pay 5 km/h there.
        let matrix = MapMatrix(vec![vec![2, 2, 2, 2]]);
        let closed = HashSet::new();
        let speed_of = |value: i64| match value {
            2 => Some(SpeedClass::VehicleAirfield),
            3 => Some(SpeedClass::VehicleStand),
            _ => None,
        };
        let plain = graph_from_grid(
            &matrix,
            &speed_of,
            &speed_of,
            &closed,
            5.0,
            55.83,
            37.63,
        );
        let overlays = |x: i64, y: i64| {
            ((x, y) == (1, 0) || (x, y) == (2, 0))
                .then(|| vec![2, 3])
                .filter(|_| y == 0)
        };
        let overlaid = graph_from_grid_with_overlays(
            &matrix,
            &speed_of,
            &speed_of,
            &overlays,
            &closed,
            5.0,
            55.83,
            37.63,
        );
        let stops = [Point::new(0, 0), Point::new(3, 0)];
        let fast = plain.route_stops(AgentKind::Vehicle, &stops).expect("plain route");
        let slow = overlaid
            .route_stops(AgentKind::Vehicle, &stops)
            .expect("overlaid route");
        assert!(
            slow.seconds > fast.seconds,
            "the co-located 5 km/h road must slow the route: {} vs {}",
            slow.seconds,
            fast.seconds
        );
        // 15 m at 20 km/h plus 10 m at 5 km/h versus 15 m and 10 m at 20 km/h.
        let expected = 15.0 / kmh_to_mps(20.0) + 10.0 / kmh_to_mps(5.0);
        assert!(
            (slow.seconds - expected).abs() < 1.0,
            "expected about {expected}s through the slow cells, got {}",
            slow.seconds
        );
    }

    #[test]
    fn walk_only_cells_are_open_to_an_engineer_and_shut_to_a_vehicle() {
        // Column 0 is a pedestrian path, column 2 is apron, column 1 is empty;
        // the two are joined at the bottom row.
        let matrix = MapMatrix(vec![
            vec![5, 0, 2],
            vec![5, 0, 2],
            vec![5, 5, 2],
        ]);
        let graph = graph_from_grid(
            &matrix,
            &|value| (value == 2).then_some(SpeedClass::VehicleApron),
            &|value| (value != 0).then_some(SpeedClass::Walk),
            &HashSet::new(),
            5.0,
            55.83,
            37.63,
        );
        let stops = [Point::new(0, 0), Point::new(2, 0)];
        assert!(
            graph.route_stops(AgentKind::Engineer, &stops).is_ok(),
            "the engineer must be able to walk the pedestrian path and step onto the apron"
        );
        // A pedestrian path is not a road: a vehicle cannot start there.
        assert!(
            graph.route_stops(AgentKind::Vehicle, &stops).is_err(),
            "a vehicle must not drive a pedestrian path"
        );
        // And the engineer may walk where a vehicle may drive.
        let apron = [Point::new(2, 0), Point::new(2, 1)];
        assert!(graph.route_stops(AgentKind::Engineer, &apron).is_ok());
        assert!(graph.route_stops(AgentKind::Vehicle, &apron).is_ok());
    }

    #[test]
    fn speed_class_names_round_trip() {
        for class in [
            SpeedClass::Walk,
            SpeedClass::VehicleStand,
            SpeedClass::VehicleTerminal,
            SpeedClass::VehicleApron,
            SpeedClass::VehicleAirfield,
            SpeedClass::Runway,
        ] {
            assert_eq!(SpeedClass::from_name(class.name()), class);
        }
        assert_eq!(SpeedClass::from_name("no-such-class"), SpeedClass::VehicleApron);
    }

    #[test]
    fn projection_flips_y_so_row_zero_is_north() {
        let matrix = MapMatrix(vec![vec![1], vec![1]]);
        let graph = graph_from_legacy_grid(
            &matrix,
            &HashSet::from([1]),
            &HashSet::new(),
            10.0,
            55.83,
            37.63,
        );
        let (lat_row0, lon0) = graph.project(0, 0);
        let (lat_row1, lon1) = graph.project(0, 1);
        assert!(lat_row0 > lat_row1, "row 0 must be north of row 1");
        assert!((lon0 - lon1).abs() < 1e-9);
        let (x, y) = graph.unproject(lat_row1, lon1);
        assert!(x.abs() < 1e-6 && (y - 1.0).abs() < 1e-6, "unproject inverts project");
    }
}
