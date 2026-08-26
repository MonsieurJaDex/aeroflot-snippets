use crate::types::*;
use std::collections::{HashMap, HashSet, VecDeque};

pub fn bfs(matrix: &MapMatrix<i64>, start: Point<i64>, end: Point<i64>) -> Route<i64> {
    if start.eq(&end) {
        return Route::new(vec![end]);
    }

    let mut q: VecDeque<Point<i64>> = VecDeque::new();
    let mut path: HashMap<Point<i64>, Point<i64>> = HashMap::new();

    q.push_back(start);
    path.insert(start, Point::new(-1, -1));

    while let Some(p) = q.pop_front() {
        let directions = [(-1, 0), (1, 0), (0, -1), (0, 1)];

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

            if path.contains_key(&next_point) {
                continue;
            }

            let _value = &row[next_x as usize];

            path.insert(next_point, p);

            if next_point.eq(&end) {
                let mut route: Vec<Point<i64>> = Vec::new();
                let mut current = next_point;

                while current.ne(&Point::new(-1, -1)) {
                    route.push(current);
                    current = path.get(&current).unwrap().clone();
                }

                route.reverse();
                return Route::new(route);
            } else {
                q.push_back(next_point);
            }
        }
    }

    return Route::new(vec![]);
}

pub fn find_nearest(
    matrix: &MapMatrix<i64>,
    start: Point<i64>,
    target_value: i64,
    road_points: &HashSet<i64>,
) -> Route<i64> {
    if start.1 < 0 || start.1 >= matrix.0.len() as i64 {
        return Route::new(vec![]);
    }
    let row_len = matrix.0[start.1 as usize].len() as i64;
    if start.0 < 0 || start.0 >= row_len {
        return Route::new(vec![]);
    }

    if matrix.0[start.1 as usize][start.0 as usize] == target_value {
        return Route::new(vec![]);
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
                return Route::new(route);
            }

            q.push_back(next_point);
        }
    }

    Route::new(vec![])
}
