use crate::types::map::Route;
use anyhow::{Ok, Result, anyhow};
use diesel::r2d2::PooledConnection;
use redis::{Client, TypedCommands};
use uuid::Uuid;

use std::collections::{HashMap, HashSet, VecDeque};

use crate::types::map::{MapMatrix, Point};

// start-to-end path finding BFS
pub fn bfs(
    matrix: &MapMatrix,
    start: Point,
    end: Point,
    road_points: &HashSet<i64>,
    redis_conn: &mut PooledConnection<Client>,
) -> Result<Route> {
    let row_len = matrix
        .0
        .get(start.0 as usize)
        .ok_or_else(|| anyhow!("row index out of matrix bound"))?
        .len();

    if start.1 < 0 || start.1 as usize >= matrix.0.len() {
        return Err(anyhow!("start y coordinate out of matrix"));
    }
    if start.0 < 0 || start.0 as usize >= row_len {
        return Err(anyhow!("start x coordinate out of matrix"));
    }

    if end.1 < 0 || end.1 as usize >= matrix.0.len() {
        return Err(anyhow!("end y coordinate out of matrix"));
    }
    if end.0 < 0 || end.0 as usize >= row_len {
        return Err(anyhow!("end x coordinate out of matrix"));
    }

    if start == end {
        return Ok(Route::new(vec![start]));
    }

    let path_key = Route::new(vec![start, end]).compute_universal_key();

    match redis_conn.get(&path_key) {
        Result::Ok(Some(cached)) => match serde_json::from_str::<Vec<Point>>(&cached) {
            Result::Ok(parsed) => return Ok(Route::new(parsed)),
            Result::Err(e) => {
                tracing::warn!(error = %e, key = %path_key, "failed to deserialize cached route, recomputing");
            }
        },
        Result::Ok(None) => {}
        Result::Err(e) => {
            tracing::warn!(error = %e, key = %path_key, "redis get failed, recomputing route");
        }
    }

    let mut q = VecDeque::new();
    let mut parent_map: HashMap<Point, Option<Point>> = HashMap::new();

    q.push_back(start);
    parent_map.insert(start, None);

    let directions = [(1, 0), (0, 1), (-1, 0), (0, -1)];

    while let Some(p) = q.pop_front() {
        for (dx, dy) in directions {
            let next_x = p.0 + dx;
            let next_y = p.1 + dy;

            if next_y < 0 || next_y as usize >= matrix.0.len() {
                continue;
            }

            let row = &matrix.0[next_y as usize];

            if next_x < 0 || next_x as usize >= row.len() {
                continue;
            }

            let next_point = Point::new(next_x, next_y);
            let value = row[next_x as usize];

            if !road_points.contains(&value) {
                continue;
            }

            if parent_map.contains_key(&next_point) {
                continue;
            }

            parent_map.insert(next_point, Some(p));

            if next_point == end {
                let mut route_points = Vec::new();
                let mut current = Some(next_point);

                while let Some(pt) = current {
                    route_points.push(pt);
                    current = *parent_map.get(&pt).unwrap_or(&None);
                }

                route_points.reverse();
                let route = Route::new(route_points);

                let value: Vec<String> = route.get_vec().iter().map(|x| x.as_value()).collect();

                match serde_json::to_string(&value) {
                    Result::Ok(serialized) => {
                        if let Err(e) = redis_conn.set(&path_key, serialized) {
                            tracing::warn!(error = %e, key = %path_key, "failed to cache computed route");
                        }
                    }
                    Result::Err(e) => {
                        tracing::warn!(error = %e, "failed to serialize route for caching");
                    }
                }

                return Ok(route);
            }

            q.push_back(next_point);
        }
    }

    Ok(Route::new(vec![]))
}

pub fn find_nearest(
    matrix: &MapMatrix,
    start: Point,
    road_points: &HashSet<i64>,
    engineer_positions: &HashMap<Point, Uuid>,
    redis_conn: &mut PooledConnection<Client>,
) -> anyhow::Result<Route> {
    if start.1 < 0 || start.1 as usize >= matrix.0.len() {
        return Err(anyhow!("start Y point out of matrix bound"));
    }
    let row_len = matrix.0[start.1 as usize].len() as i64;
    if start.0 < 0 || start.0 >= row_len {
        return Err(anyhow!("start X point out of matrix bound"));
    }

    if engineer_positions.get(&start).is_some() {
        return Ok(Route::new(vec![]));
    }

    let mut q = VecDeque::new();
    let mut parent_map: HashMap<Point, Option<Point>> = HashMap::new();

    q.push_back(start);
    parent_map.insert(start, None);

    let directions = [(1, 0), (0, 1), (-1, 0), (0, -1)];

    while let Some(p) = q.pop_front() {
        for (dx, dy) in directions {
            let next_x = p.0 + dx;
            let next_y = p.1 + dy;

            if next_y < 0 || next_y as usize >= matrix.0.len() {
                continue;
            }

            let row = &matrix.0[next_y as usize];

            if next_x < 0 || next_x as usize >= row.len() {
                continue;
            }

            let next_point = Point::new(next_x, next_y);
            let value = row[next_x as usize];

            let is_target = engineer_positions.contains_key(&next_point);

            if !is_target && !road_points.contains(&value) {
                continue;
            }

            if parent_map.contains_key(&next_point) {
                continue;
            }

            parent_map.insert(next_point, Some(p));

            if is_target {
                let mut route = Vec::new();
                let mut current = Some(next_point);

                while let Some(pt) = current {
                    route.push(pt);
                    current = *parent_map.get(&pt).unwrap_or(&None);
                }

                let route = Route::new(route);

                let value: Vec<String> = route.get_vec().iter().map(|x| x.as_value()).collect();

                match serde_json::to_string(&value) {
                    Result::Ok(serialized) => {
                        if let Err(e) = redis_conn.set(&route.compute_universal_key(), serialized) {
                            tracing::warn!(error = %e, "failed to cache find_nearest route");
                        }
                    }
                    Result::Err(e) => {
                        tracing::warn!(error = %e, "failed to serialize route for caching");
                    }
                }

                return Ok(route);
            }

            q.push_back(next_point);
        }
    }

    Ok(Route::new(vec![]))
}
