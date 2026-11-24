//! Road graph with per-edge speed and agent access.
//!
//! A cell map becomes a graph of walkable cells. A point that is not on a
//! node is snapped to the nearest open edge the agent is allowed to use.
//! Every route is a time-weighted A* search on this graph.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::types::map::Point;

pub const WALK_KMH: f64 = 5.0;
pub const VEHICLE_STAND_KMH: f64 = 5.0;
pub const VEHICLE_TERMINAL_KMH: f64 = 8.0;
pub const VEHICLE_APRON_KMH: f64 = 10.0;
pub const VEHICLE_AIRFIELD_KMH: f64 = 20.0;
pub const TOOL_DELAY_SECS: f64 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Engineer,
    Vehicle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedClass {
    Walk,
    VehicleStand,
    VehicleTerminal,
    VehicleApron,
    VehicleAirfield,
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

/// Vehicle speed at a cell for an edge class, metres per second.
pub fn vehicle_speed_mps(_at: Point, class: SpeedClass) -> f64 {
    let kmh = match class {
        SpeedClass::Walk => walk_speed_kmh(),
        SpeedClass::VehicleStand => vehicle_stand_speed_kmh(),
        SpeedClass::VehicleTerminal => vehicle_terminal_speed_kmh(),
        SpeedClass::VehicleApron => vehicle_apron_speed_kmh(),
        SpeedClass::VehicleAirfield => vehicle_airfield_speed_kmh(),
    };
    kmh_to_mps(kmh)
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

#[derive(Debug, Clone)]
pub struct PlannedRoute {
    pub points: Vec<Point>,
    pub seconds: f64,
}

impl RoadGraph {
    #[allow(dead_code)]
    pub fn project(&self, x: i64, y: i64) -> (f64, f64) {
        let north = (y as f64) * self.cell_size_m / 111_320.0;
        let lat = self.origin_lat + north;
        let cos = (self.origin_lat.to_radians()).cos().max(0.2);
        let east = (x as f64) * self.cell_size_m / (111_320.0 * cos);
        (lat, self.origin_lon + east)
    }

    pub fn unproject(&self, lat: f64, lon: f64) -> (f64, f64) {
        let y = (lat - self.origin_lat) * 111_320.0 / self.cell_size_m;
        let cos = (self.origin_lat.to_radians()).cos().max(0.2);
        let x = (lon - self.origin_lon) * 111_320.0 * cos / self.cell_size_m;
        (x, y)
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
            });
        }
        let mut points = Vec::new();
        let mut seconds = 0.0;
        for pair in stops.windows(2) {
            let leg = self.route_leg(agent, pair[0], pair[1])?;
            if points.is_empty() {
                points.extend(leg.points);
            } else if leg.points.len() > 1 {
                points.extend_from_slice(&leg.points[1..]);
            }
            seconds += leg.seconds;
        }
        Ok(PlannedRoute { points, seconds })
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
        let mut seconds = approach / approach_speed + departure / departure_speed;

        if start_id == end_id {
            let mut points = vec![start, Point::new(start_node.x, start_node.y)];
            if end != points[points.len() - 1] {
                points.push(end);
            }
            return Ok(PlannedRoute { points, seconds });
        }

        let (path, travel) = self.astar(agent, start_id, end_id)?;
        seconds += travel;
        let mut points = Vec::with_capacity(path.len() + 2);
        if start != Point::new(start_node.x, start_node.y) {
            points.push(start);
        }
        for id in path {
            let node = &self.nodes[id as usize];
            points.push(Point::new(node.x, node.y));
        }
        if end != *points.last().unwrap_or(&end) {
            points.push(end);
        }
        Ok(PlannedRoute { points, seconds })
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

/// Walkable cells become nodes. Four-neighbour links are open roads.
pub fn graph_from_grid(
    matrix: &crate::types::map::MapMatrix,
    road_values: &HashSet<i64>,
    closed: &HashSet<(i64, i64)>,
    cell_size_m: f64,
    origin_lat: f64,
    origin_lon: f64,
) -> RoadGraph {
    let mut builder = GraphBuilder::new(cell_size_m, origin_lat, origin_lon);
    for (y, row) in matrix.0.iter().enumerate() {
        for (x, value) in row.iter().enumerate() {
            if road_values.contains(value) {
                builder.add_node(x as i64, y as i64);
            }
        }
    }
    let coords: Vec<(i64, i64)> = builder.index.keys().copied().collect();
    for (x, y) in coords {
        let from = builder.index[&(x, y)];
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let nx = x + dx;
            let ny = y + dy;
            let Some(&to) = builder.index.get(&(nx, ny)) else {
                continue;
            };
            let blocked = closed.contains(&(x, y)) || closed.contains(&(nx, ny));
            builder.add_edge(
                from,
                to,
                cell_size_m,
                SpeedClass::VehicleApron,
                true,
                true,
                blocked,
            );
        }
    }
    builder.finish()
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
        let graph = graph_from_grid(&matrix, &roads, &HashSet::new(), 10.0, 55.83, 37.63);
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
}
