//! The v1 files must keep loading, with a palette synthesised from `road`.

use std::path::Path;

#[test]
fn legacy_map2_synthesises_a_palette_from_road() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives in <root>/tools/parser")
        .to_path_buf();
    let path = root.join("backend/assets/map2.json");
    let raw = std::fs::read_to_string(&path).expect("map2.json");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("json");
    let doc = parser::types::MapDocument::from_json(value).expect("v1 upgrades");
    assert_eq!(doc.format, 2);
    assert_eq!(doc.width, 64);
    assert_eq!(doc.height, 64);
    assert!(!doc.palette.is_empty());
    // The three TileD gids of the placeholder map become palette entries.
    let ids: std::collections::BTreeSet<i64> = doc.palette.iter().map(|p| p.id).collect();
    for gid in [973, 974, 975] {
        assert!(ids.contains(&gid), "gid {gid} must become a palette entry");
    }
    doc.validate().expect("upgraded v1 document validates");
}

#[test]
fn v1_upgrade_keeps_road_ids_unchanged() {
    let raw = r#"{"map":[[0,973,974],[0,975,0]],"road":[973,974,975]}"#;
    let value: serde_json::Value = serde_json::from_str(raw).expect("json");
    let doc = parser::types::MapDocument::from_json(value).expect("v1 upgrades");
    assert_eq!(doc.width, 3);
    assert_eq!(doc.height, 2);
    assert_eq!(doc.road_ids(), std::collections::HashSet::from([973, 974, 975]));
    assert_eq!(doc.walk_ids(), std::collections::HashSet::from([973, 974, 975]));
    assert_eq!(doc.max_spd_kmh(), 10.0, "v1 cells stay apron-speed");
    assert_eq!(doc.map.0[0][1], 973);
}

#[test]
fn v1_json_roundtrips_through_the_legacy_struct() {
    // The TMJ path still writes the old shape, so reading it back must work.
    let legacy = parser::parser::JsonMatrix {
        map: parser::types::MapMatrix(vec![vec![1, 2], vec![0, 1]]),
        road: vec![1, 2],
    };
    let text = serde_json::to_string(&legacy).expect("serialise");
    let back: parser::parser::JsonMatrix = serde_json::from_str(&text).expect("deserialise");
    assert_eq!(back.road, vec![1, 2]);
    assert_eq!(back.map.0[1][0], 0);
}
