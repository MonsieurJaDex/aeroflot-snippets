use std::collections::HashSet;

mod bfs;
mod error;
mod parser;
mod types;

fn main() {
    // let map = parser::parse_map::<i64>("./assets/map.tmj", Some("./assets/parsed/map.json"));
    let map = parser::parse_from_json::<i64>("./assets/parsed/map.json");

    let map = match map {
        Ok(m) => m,
        Err(e) => {
            println!("{}", e.to_string());
            return;
        }
    };

    let roads: HashSet<i64> = HashSet::from([
        2684354912, 2684355023, 2684355024, 2684354967, 2684354886, 3221225935, 3221225936,
        3221225879, 3221225798, 1610613200, 1610613199, 1610613143, 29,
    ]);
    let route = bfs::find_nearest(&map, types::Point::new(0, 0), 407, &roads);

    println!("{:?}", map);
}
