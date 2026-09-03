use std::{
    collections::HashSet,
    env,
    net::{AddrParseError, Ipv4Addr},
    str::FromStr,
};

use anyhow::{Context, Result, anyhow};
use num_traits::PrimInt;

use crate::types::map::{MapMatrix, Point};

pub struct AppConfig {
    pub host: Ipv4Addr,
    pub port: u16,
    pub debug: bool,
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
        dotenvy::dotenv()?;

        let host: Ipv4Addr = AppConfig::parse_env("host")?;
        let port: u16 = AppConfig::parse_env("port")?;
        let debug: bool = AppConfig::parse_env("debug")?;

        anyhow::Result::Ok(Self { host, port, debug })
    }
}

#[derive(Debug, Clone)]
pub struct AppState<T: PrimInt> {
    pub road_points: HashSet<T>,
    pub map: MapMatrix<T>,
}
