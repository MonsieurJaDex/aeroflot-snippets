//! Loads an airport map file into a [`MapDocument`].
//!
//! Two on-disk generations are accepted, distinguished by the `format` field:
//!
//! - v2 (`format: 2`) — the palette matrix written by `tools/parser`
//!   (`backend/assets/MAP_FORMAT_V2.md`). Overlapping layers survive as
//!   separate palette entries and speed sections as separate entries, while
//!   `map` stays the rectangular `Vec<Vec<i64>>` the frontend renders.
//! - v1 (`{ "map": [[i64]], "road": [i64] }`) — the TileD-derived shape. It is
//!   upgraded in memory: the palette is synthesised from `road`, so a v1 map
//!   routes exactly as it did before this format existed.
//!
//! A `.tmj` file is read through Tiled's layer semantics as a last resort.

use std::path::Path;

use anyhow::{Context, anyhow, bail};

use crate::types::map::{MapDocument, MapMatrix};

/// Tiled layer subset needed to flatten a `.tmj`.
#[derive(serde::Deserialize)]
struct TiledLayer {
    data: Vec<i64>,
    /// Kept because Tiled writes it; the flatten uses the map dimensions and
    /// ignores a layer taller than the map.
    #[allow(dead_code)]
    height: usize,
    width: usize,
}

#[derive(serde::Deserialize)]
struct TiledMap {
    width: usize,
    height: usize,
    layers: Vec<TiledLayer>,
}

/// Reads a map file of any supported generation.
///
/// # Errors
/// The file cannot be read, is not JSON, or violates the format invariants.
pub fn load_map_document(path: &Path) -> anyhow::Result<MapDocument> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read map {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("map {} is not JSON", path.display()))?;

    // `map` + `road` present with a matrix of gids and no palette: v1 or v2,
    // both handled by `from_json`.
    if value.get("map").is_some() {
        return MapDocument::from_json(value)
            .map_err(|error| anyhow!("map {}: {error}", path.display()));
    }

    // TileD: flatten layers exactly as the old loader did, then treat the
    // result as a v1 document.
    let tiled: TiledMap = serde_json::from_value(value)
        .map_err(|error| anyhow!("map {}: {error}", path.display()))?;
    if tiled.width == 0 || tiled.height == 0 {
        bail!("map {} has a zero dimension", path.display());
    }
    let mut matrix = vec![vec![0_i64; tiled.width]; tiled.height];
    for layer in tiled.layers {
        if layer.width == 0 {
            continue;
        }
        for (index, tile) in layer.data.into_iter().enumerate() {
            let (row, column) = (index / layer.width, index % layer.width);
            if row >= tiled.height || column >= tiled.width || tile == 0 {
                continue;
            }
            matrix[row][column] = tile;
        }
    }
    let road: Vec<i64> = matrix
        .iter()
        .flatten()
        .copied()
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .filter(|value| *value != 0)
        .collect();
    let legacy = crate::types::map::JsonMatrix {
        map: MapMatrix(matrix),
        road,
    };
    let value = serde_json::to_value(&legacy)?;
    MapDocument::from_json(value).map_err(|error| anyhow!("map {}: {error}", path.display()))
}
