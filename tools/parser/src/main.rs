use std::process;

use parser::{
    parser::{init_config_file, parse_geojson, parse_tmj, upgrade_v1},
    types::{AppConfig, Config, resolve},
};

fn load_config(app_config: &AppConfig) -> Result<Config, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(&app_config.config_filepath)?;
    let config: Config = serde_json::from_str(&raw)?;
    Ok(config)
}

/// Which builder `parse` must run.
///
/// `source` is authoritative when it says `geojson`. Otherwise (the legacy
/// default, `tmj`) the TMJ input decides: a `map_path` that names a readable
/// file means TMJ, and an empty or missing one with a resolvable geodata
/// directory means GeoJSON. Only when neither input exists is it an error —
/// reading the empty `map_path` as a file is what produced
/// `Is a directory (os error 21)`.
fn uses_geojson(config: &Config) -> Result<bool, String> {
    if config.source.eq_ignore_ascii_case("geojson") {
        return Ok(true);
    }
    if !config.map_path.trim().is_empty() {
        let path = resolve(&config.map_path);
        if path.is_file() {
            return Ok(false);
        }
        return Err(format!(
            "map_path {:?} is not a file (resolved to {})",
            config.map_path,
            path.display()
        ));
    }
    let dir = config.input_path();
    if dir.is_dir() {
        return Ok(true);
    }
    Err(format!(
        "no map input: set `source: \"geojson\"` with an `input_dir` (tried {}), \
         or `map_path` to a .tmj file",
        dir.display()
    ))
}

fn main() {
    let args = std::env::args().collect::<Vec<String>>();
    let app_config = AppConfig::new(None, None).unwrap();

    if args.len() != 2 {
        println!("Usage: ./parser [init/parse/upgrade]");
        process::exit(1);
    }

    let cmd = args.get(1).unwrap();
    let res: Result<(), Box<dyn std::error::Error>> = match cmd.as_str() {
        "init" => init_config_file(&app_config).map_err(Into::into),
        "parse" => load_config(&app_config).and_then(|config| {
            let geojson = uses_geojson(&config)?;
            if geojson {
                parse_geojson(&config).map_err(Into::into)
            } else {
                parse_tmj(&config, &app_config).map_err(Into::into)
            }
        }),
        "upgrade" => load_config(&app_config).and_then(|config| upgrade_v1(&config).map_err(Into::into)),
        _ => {
            println!("Invalid argument: {}", cmd);
            process::exit(1);
        }
    };

    match res {
        Ok(()) => println!("Success!"),
        Err(err) => {
            println!("Error has ocurred: {err}");
            process::exit(1);
        }
    }
}
