//! The TileD path must keep working: a `.tmj` still flattens to a walkable
//! matrix, and a v1 file still upgrades to a v2 document.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives in <root>/tools/parser")
        .to_path_buf()
}

#[test]
fn tmj_flattens_every_tile_layer_and_skips_object_groups() {
    let root = repo_root();
    let out_dir = root.join("tools/parser/target/tmj-test");
    std::fs::create_dir_all(&out_dir).expect("scratch dir");
    let out = out_dir.join("vdnh.json");

    let raw = std::fs::read_to_string(root.join("tools/parser/config.json")).expect("config");
    let mut config: parser::types::Config = serde_json::from_str(&raw).expect("config parses");
    config.source = "tmj".into();
    config.map_path = format!("{}/backend/assets/vdnh.tmj", root.display());
    config.roads = vec![973, 974, 975];
    config.out = out.to_string_lossy().into_owned();

    let app = parser::types::AppConfig::new(Some(root.join("tools/parser/config.json")), Some(out.clone()))
        .expect("app config");
    parser::parser::parse_tmj(&config, &app).expect("tmj parses");

    let written = std::fs::read_to_string(&out).expect("v1 output");
    let value: serde_json::Value = serde_json::from_str(&written).expect("json");
    let matrix = value["map"].as_array().expect("map");
    assert!(matrix.len() > 64, "the real city map is big");
    assert_eq!(value["road"], serde_json::json!([973, 974, 975]));
    // The object group in vdnh.tmj has no `data`; if it were not skipped the
    // whole parse would fail, so reaching this line is the regression proof.
    let seen: std::collections::BTreeSet<i64> = matrix
        .iter()
        .flat_map(|row| row.as_array().unwrap().iter())
        .map(|cell| cell.as_i64().unwrap())
        .collect();
    assert!(seen.contains(&973), "gids must be preserved, saw {:?}", &seen);

    // And that v1 file still upgrades into a valid v2 document.
    let doc = parser::types::MapDocument::from_json(value).expect("v1 upgrades");
    doc.validate().expect("upgraded document validates");
    assert_eq!(doc.road_ids(), std::collections::HashSet::from([973, 974, 975]));
    std::fs::remove_dir_all(&out_dir).ok();
}
