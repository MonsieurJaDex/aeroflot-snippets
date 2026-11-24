use std::process;

use parser::{
    parser::{init_config_file, parse_geojson, parse_tmj, upgrade_v1},
    types::{AppConfig, Config},
};

fn load_config(app_config: &AppConfig) -> Result<Config, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(&app_config.config_filepath)?;
    let config: Config = serde_json::from_str(&raw)?;
    Ok(config)
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
            if config.source.eq_ignore_ascii_case("geojson") {
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
