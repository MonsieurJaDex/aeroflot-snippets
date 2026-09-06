use std::{collections::HashSet, env, net::Ipv4Addr, path, str::FromStr};

use anyhow::{Context, Result};
use diesel::{
    PgConnection,
    r2d2::{ConnectionManager, Pool},
};

use clap::Parser;

use crate::types::map::MapMatrix;

#[derive(Debug, Parser)]
#[command(
    name = "aeroflot",
    about = "simple CLI that wraps backend running functionality"
)]
pub struct Args {
    #[arg(short = 'p', long)]
    pub map_path: String,
}

impl Args {
    pub fn validate_path(&self) -> bool {
        let path = path::Path::new(&self.map_path);
        path.is_file()
    }
}

pub struct AppConfig {
    pub host: Ipv4Addr,
    pub port: u16,
    pub debug: bool,
    pub database_url: String,
}

impl AppConfig {
    fn parse_env<T>(key: &str) -> Result<T>
    where
        T: FromStr,
        T::Err: std::error::Error + Send + Sync + 'static,
    {
        env::var(key)
            .with_context(|| format!("missing enviroment variable: '{}'", key))?
            .parse()
            .with_context(|| format!("failed to parse variable '{}'", key))
    }

    pub fn new() -> Result<Self> {
        _ = dotenvy::dotenv();

        let host: Ipv4Addr = AppConfig::parse_env("host")?;
        let port: u16 = AppConfig::parse_env("port")?;
        let debug: bool = AppConfig::parse_env("debug")?;
        let database_url: String = AppConfig::parse_env("database_url")?;

        anyhow::Result::Ok(Self {
            host,
            port,
            debug,
            database_url,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub road_points: HashSet<i64>,
    pub map: MapMatrix,
    pub db_pool: Pool<ConnectionManager<PgConnection>>,
}
