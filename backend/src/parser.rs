use std::{fs::File, io::BufReader};

use crate::types::map::JsonMatrix;

pub fn parse_from_json(path: &str) -> Result<JsonMatrix, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let json_matrix: JsonMatrix = serde_json::from_reader(reader)?;

    Ok(json_matrix)
}
