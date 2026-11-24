//! The command line must reach the GeoJSON builder.
//!
//! The regression this pins: `config.json` carried `source: "tmj"` with an empty
//! `map_path`, so `parse` resolved the empty path to the working directory and
//! failed with `Is a directory (os error 21)` while the library path stayed
//! green. `parse` now decides the mode from the inputs it is actually given.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives in <root>/tools/parser")
        .to_path_buf()
}

fn parser_bin() -> PathBuf {
    // Built by cargo for this test run.
    let exe = std::env::current_exe().expect("test binary path");
    let dir = exe.parent().expect("deps dir").parent().expect("target dir");
    dir.join("parser")
}

#[test]
fn committed_config_is_in_geojson_mode() {
    let raw = std::fs::read_to_string(repo_root().join("tools/parser/config.json"))
        .expect("config.json");
    let cfg: parser::types::Config = serde_json::from_str(&raw).expect("config parses");
    assert_eq!(
        cfg.source.to_lowercase(),
        "geojson",
        "the committed config must build the airport map, not the empty TMJ stub"
    );
    assert!(
        !cfg.out.is_empty() && !cfg.location_out.is_empty(),
        "an artefact run must have both outputs set"
    );
    assert!(
        cfg.input_path().is_dir(),
        "input_dir {} must exist",
        cfg.input_path().display()
    );
}

/// A copy of the committed config whose outputs go to a scratch directory, so a
/// test run cannot rewrite the artefact under test.
fn scratch_config(root: &Path, name: &str) -> PathBuf {
    let raw = std::fs::read_to_string(root.join("tools/parser/config.json")).expect("config");
    let mut cfg: parser::types::Config = serde_json::from_str(&raw).expect("config parses");
    let dir = root.join("tools/parser/target/cli-test");
    std::fs::create_dir_all(&dir).expect("scratch");
    cfg.out = dir.join(format!("{name}.json")).display().to_string();
    cfg.location_out = dir.join(format!("{name}.location.json")).display().to_string();
    cfg.runtime_from = String::new();
    let path = dir.join(format!("{name}.config.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&cfg).expect("serialise")).expect("write");
    path
}

#[test]
fn parse_runs_from_the_parser_directory_and_from_the_repo_root() {
    let root = repo_root();
    let bin = parser_bin();
    assert!(bin.is_file(), "parser binary missing at {}", bin.display());

    for (dir, name) in [
        (root.join("tools/parser"), "from-parser-dir"),
        (root.clone(), "from-repo-root"),
    ] {
        let config = scratch_config(&root, name);
        let out = Command::new(&bin)
            .arg("parse")
            .env("PARSER_CONFIG", &config)
            .current_dir(&dir)
            .output()
            .expect("run parser");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "parse failed in {}: {stdout}{stderr}",
            dir.display()
        );
        assert!(
            !stdout.contains("Is a directory"),
            "parse read a directory as a map file: {stdout}"
        );
        assert!(
            stdout.contains("Success!"),
            "parse printed no success in {}: {stdout}",
            dir.display()
        );
        let written = root.join(format!("tools/parser/target/cli-test/{name}.json"));
        let raw = std::fs::read_to_string(&written).expect("map written");
        let doc: parser::types::MapDocument = serde_json::from_str(&raw).expect("valid v2 map");
        doc.validate().expect("written map validates");
        assert_eq!(doc.format, 2);
        assert!(doc.width > 400 && doc.height > 300, "{}x{}", doc.width, doc.height);
        assert!(!doc.overlays.is_empty(), "the real map has overlapping cells");
    }
}

#[test]
fn init_never_overwrites_an_existing_config() {
    let root = repo_root();
    let bin = parser_bin();
    let dir = root.join("tools/parser/target/init-test");
    std::fs::create_dir_all(&dir).expect("scratch");
    let cfg_path = dir.join("config.json");
    let original = std::fs::read_to_string(root.join("tools/parser/config.json")).expect("config");
    std::fs::write(&cfg_path, &original).expect("seed");

    let out = Command::new(&bin)
        .arg("init")
        .env("PARSER_CONFIG", &cfg_path)
        .current_dir(&dir)
        .output()
        .expect("run init");
    assert!(out.status.success(), "init must succeed");
    let after = std::fs::read_to_string(&cfg_path).expect("config still there");
    assert_eq!(
        after, original,
        "init must leave a working config alone instead of resetting it to the TMJ stub"
    );
}
