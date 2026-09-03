use crate::types::map::Route;
use anyhow::{Ok, Result, anyhow};
use num_traits::PrimInt;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
};

use crate::types::map::{MapMatrix, Point};

// start-to-end path finding BFS
pub fn bfs<T>(
    matrix: &MapMatrix<T>,
    start: Point<T>,
    end: Point<T>,
    road_points: &HashSet<T>,
) -> Result<Route<T>>
where
    T: PrimInt + Copy + Hash,
{
    let row_len = matrix.0[start.y.to_usize().unwrap()].len() as i64;

    if start.y < T::zero() || start.y >= T::from(matrix.0.len()).unwrap() {
        return Err(anyhow!("start y coordinate out of matrix"));
    }
    if start.x < T::zero() || start.x >= T::from(row_len).unwrap() {
        return Err(anyhow!("start x coordinate out of matrix"));
    }

    if end.y < T::zero() || end.y >= T::from(matrix.0.len()).unwrap() {
        return Err(anyhow!("end y coordinate out of matrix"));
    }
    if end.x < T::zero() || end.x >= T::from(row_len).unwrap() {
        return Err(anyhow!("end x coordinate out of matrix"));
    }

    if start == end {
        return Ok(Route::new(vec![start]));
    }

    let mut q = VecDeque::new();
    let mut parent_map: HashMap<Point<T>, Option<Point<T>>> = HashMap::new();

    q.push_back(start);
    parent_map.insert(start, None);

    let directions = [(1, 0), (0, 1), (-1, 0), (0, -1)];

    while let Some(p) = q.pop_front() {
        for (dx, dy) in directions {
            let next_x = p.x + T::from(dx).unwrap();
            let next_y = p.y + T::from(dy).unwrap();

            if next_y < T::zero() || next_y >= T::from(matrix.0.len()).unwrap() {
                continue;
            }

            let row = &matrix.0[next_y.to_usize().unwrap()];

            if next_x < T::zero() || next_x >= T::from(row.len()).unwrap() {
                continue;
            }

            let next_point = Point::new(next_x, next_y);
            let value = row[next_x.to_usize().unwrap()];

            if !road_points.contains(&value) {
                continue;
            }

            if parent_map.contains_key(&next_point) {
                continue;
            }

            parent_map.insert(next_point, Some(p));

            if next_point == end {
                let mut route = Vec::new();
                let mut current = Some(next_point);

                while let Some(pt) = current {
                    route.push(pt);
                    current = *parent_map.get(&pt).unwrap_or(&None);
                }

                route.reverse();
                return Ok(Route::new(route));
            }

            q.push_back(next_point);
        }
    }

    Ok(Route::new(vec![]))
}

pub fn find_nearest<T>(
    matrix: &MapMatrix<T>,
    start: Point<T>,
    target_value: T,
    road_points: &HashSet<T>,
) -> Route<T>
where
    T: PrimInt + Copy + Hash,
{
    if start.y < T::zero() || start.y >= T::from(matrix.0.len()).unwrap() {
        return Route::new(vec![]);
    }
    let row_len = matrix.0[start.y.to_usize().unwrap()].len() as i64;
    if start.x < T::zero() || start.x >= T::from(row_len).unwrap() {
        return Route::new(vec![]);
    }

    if matrix.0[start.y.to_usize().unwrap()][start.x.to_usize().unwrap()] == target_value {
        return Route::new(vec![]);
    }

    let mut q = VecDeque::new();
    let mut parent_map: HashMap<Point<T>, Option<Point<T>>> = HashMap::new();

    q.push_back(start);
    parent_map.insert(start, None);

    let directions = [(1, 0), (0, 1), (-1, 0), (0, -1)];

    while let Some(p) = q.pop_front() {
        for (dx, dy) in directions {
            let next_x = p.x + T::from(dx).unwrap();
            let next_y = p.y + T::from(dy).unwrap();

            if next_y < T::zero() || next_y >= T::from(matrix.0.len()).unwrap() {
                continue;
            }

            let row = &matrix.0[next_y.to_usize().unwrap()];

            if next_x < T::zero() || next_x >= T::from(row.len()).unwrap() {
                continue;
            }

            let next_point = Point::new(next_x, next_y);
            let value = row[next_x.to_usize().unwrap()];

            let is_target = value == target_value;

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

                route.reverse();
                return Route::new(route);
            }

            q.push_back(next_point);
        }
    }

    Route::new(vec![])
}
