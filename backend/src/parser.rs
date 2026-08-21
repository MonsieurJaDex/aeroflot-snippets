use std::{error::Error, fs};

use crate::types;

// map parser (tmj -> MapMatrix)
fn parse_map<T>(path: &str) -> Result<types::MapMatrix<T>, Box<dyn Error>> {
    let path = path.trim();
    let content = fs::read_to_string(path);

    if content.is_err() {
        return Err(Box::new(content.err().unwrap()));
    }

    unimplemented!()
}
