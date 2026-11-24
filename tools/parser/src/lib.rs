//! Map builder: TileD `.tmj` and airport GeoJSON -> the project's map matrix.
//!
//! The library exists so the acceptance tests can drive the builder; the
//! binary in `main.rs` is the command-line front end.

pub mod geo;
pub mod parser;
pub mod types;
