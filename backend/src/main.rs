use std::collections::HashSet;

mod bfs;
mod error;
mod parser;
mod types;

fn main() {
    let map = parser::parse_map::<i64>("./assets/map.tmj").unwrap();
    let roads: HashSet<i64> = HashSet::from([29, 0]);
    let route = bfs::find_nearest(&map, types::Point::new(0, 0), 407, &roads);

    println!("{:?}", route);
}
