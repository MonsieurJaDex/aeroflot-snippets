use crate::types::*;
use std::collections::{HashMap, VecDeque};


// start-to-end path finding BFS
pub fn bfs(matrix: &MapMatrix<i32>, start: Point<i32>, end: Point<i32>) -> Vec<Point<i32>> {
    if start.eq(&end) {
        return vec![end];
    }

    let mut q: VecDeque<Point<i32>> = VecDeque::new();
    let mut path: HashMap<Point<i32>, Point<i32>> = HashMap::new();

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
            if next_y < 0 || next_y >= matrix.len() as i32 {
                continue;
            }

            let row = &matrix[next_y as usize];

            // if x out of row's range
            if next_x < 0 || next_x >= row.len() as i32 {
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
                let mut route: Vec<Point<i32>> = Vec::new();
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


pub fn find_nearest(matrix: &MapMatrix<i32>, target: Point<i32>, target_value: i32) -> Vec<Point<i32>> {
    let mut q = VecDeque::<Point<i32>>::new();
    let mut path = HashMap::<Point<i32>, Point<i32>>::new();
    let mut route = Vec::<Point<i32>>::new();

    q.push_back(target);
    path.insert(target, Point::new(-1, -1));

    while let Some(p) = q.pop_front() {
        let directions = [(1, 0), (0, 1), (-1, 0), (0, -1)];

        for (dx, dy) in directions {
            let next_x = p.0 + dx;
            let next_y = p.1 + dy;

            // if row out of range
            if next_y < 0 || next_y >= matrix.len() as i32 {
                continue;
            }

            let row = &matrix[next_y as usize];

            // if x out of row's range
            if next_x < 0 || next_x >= row.len() as i32 {
                continue;
            }

            let next_point = Point::new(next_x, next_y); // next point object

            if path.contains_key(&next_point) {
                continue;
            }

            path.insert(next_point, p);

            // TODO: use value for roads
            let value = &row[next_x as usize]; // value of next point
            
            if value.ne(&target_value) {
                q.push_back(next_point);
                continue;
            }

            // route recovery
            let mut current = next_point;
            while current.ne(&Point::new(-1, -1)) {
                route.push(current);
                current = *path.get(&current).unwrap();
            }

            route.reverse();
            return route;
        }
    }

    return route;
}

