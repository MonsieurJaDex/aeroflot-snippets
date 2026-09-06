use std::process;

use crate::{
    parser::{init_config_file, parse_map},
    types::AppConfig,
};

mod parser;
mod types;

fn main() {
    let args = std::env::args().collect::<Vec<String>>();
    let app_config = AppConfig::new(None, None).unwrap();

    if args.len() != 2 {
        println!("Usage: ./parser [init/parse]");
        process::exit(1);
    }

    let cmd = args.get(1).unwrap();
    let res = match cmd.as_str() {
        "init" => init_config_file(&app_config),
        "parse" => parse_map(&app_config),
        _ => {
            println!("Invalid argument: {}", cmd);
            process::exit(1);
        }
    };

    if res.is_err() {
        println!("Error has ocurred: {}", res.err().unwrap().to_string());
    } else {
        println!("File parsed successfully!");
    }
}
