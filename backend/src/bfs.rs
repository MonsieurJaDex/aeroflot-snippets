use num_traits::PrimInt;

use crate::types::*;
use std::collections::{HashMap, HashSet, VecDeque};

// start-to-end path finding BFS
pub fn bfs(matrix: &MapMatrix<i64>, start: Point<i64>, end: Point<i64>) -> Vec<Point<i64>> {
    if start.eq(&end) {
        return vec![end];
    }

    let mut q: VecDeque<Point<i64>> = VecDeque::new();
    let mut path: HashMap<Point<i64>, Point<i64>> = HashMap::new();

    q.push_back(start);
    path.insert(start, Point::new(-1, -1));

    while let Some(p) = q.pop_front() {
        let directions = [(-1, 0), (1, 0), (0, -1), (0, 1)];

        // iterate over 4 directions with respect to current point
        for (dx, dy) in directions {
            // evaluate next point's coords
            let next_x = p.0 + dx;
            let next_y = p.1 + dy;

            // if row out of range
            if next_y < 0 || next_y >= matrix.0.len() as i64 {
                continue;
            }

            let row = &matrix.0[next_y as usize];

            // if x out of row's range
            if next_x < 0 || next_x >= row.len() as i64 {
                continue;
            }

            let next_point = Point::new(next_x, next_y); // next point object

            if path.contains_key(&next_point) {
                continue;
            }

            // TODO: use value for roads
            let value = &row[next_x as usize]; // value of next point

            path.insert(next_point, p);

            if next_point.eq(&end) {
                let mut route: Vec<Point<i64>> = Vec::new();
                let mut current = next_point;

                while current.ne(&Point::new(-1, -1)) {
                    route.push(current);
                    current = path.get(&current).unwrap().clone();
                }

                route.reverse();
                return route;
            } else {
                q.push_back(next_point);
            }
        }
    }

    return vec![];
}

// Предполагается, что где-то определены:
// pub struct Point<T> { pub x: T, pub y: T } или pub struct Point<T>(pub T, pub T);
// pub struct MapMatrix<T>(pub Vec<Vec<T>>);

pub fn find_nearest(
    matrix: &MapMatrix<i64>,
    start: Point<i64>,
    target_value: i64,
    road_points: &HashSet<i64>,
) -> Vec<Point<i64>> {
    if start.1 < 0 || start.1 >= matrix.0.len() as i64 {
        return vec![];
    }
    let row_len = matrix.0[start.1 as usize].len() as i64;
    if start.0 < 0 || start.0 >= row_len {
        return vec![];
    }

    if matrix.0[start.1 as usize][start.0 as usize] == target_value {
        return vec![start];
    }

    let mut q = VecDeque::new();
    let mut parent_map: HashMap<Point<i64>, Option<Point<i64>>> = HashMap::new();

    q.push_back(start);
    parent_map.insert(start, None);

    let directions = [(1, 0), (0, 1), (-1, 0), (0, -1)];

    while let Some(p) = q.pop_front() {
        for (dx, dy) in directions {
            let next_x = p.0 + dx;
            let next_y = p.1 + dy;

            if next_y < 0 || next_y >= matrix.0.len() as i64 {
                continue;
            }

            let row = &matrix.0[next_y as usize];

            if next_x < 0 || next_x >= row.len() as i64 {
                continue;
            }

            let next_point = Point::new(next_x, next_y);
            let value = row[next_x as usize];

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
                return route;
            }

            q.push_back(next_point);
        }
    }

    vec![]
}
