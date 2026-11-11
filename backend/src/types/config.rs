//! Application configuration and shared runtime state.
//!
//! The configuration is sourced from environment variables (with optional
//! `.env` file support via [`dotenvy`]) and combined with a parsed JSON map
//! into an [`AppState`] that is cloned into every HTTP handler.

use std::{collections::HashSet, env, net::Ipv4Addr, path, str::FromStr};

use anyhow::{Context, Result};
use diesel::{
    PgConnection,
    r2d2::{ConnectionManager, Pool},
};

use clap::Parser;
use redis::Client;

use crate::types::map::MapMatrix;

/// Command line arguments for the backend binary.
#[derive(Debug, Parser)]
#[command(
    name = "aeroflot",
    about = "Simple CLI wrapper that runs the backend server"
)]
pub struct Args {
    /// Filesystem path to the airport map (JSON file).
    #[arg(short = 'p', long)]
    pub map_path: String,
}

impl Args {
    /// Returns `true` if the configured map path points to an existing file.
    #[must_use]
    pub fn validate_path(&self) -> bool {
        path::Path::new(&self.map_path).is_file()
    }
}

/// Application-wide configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Bind address for the HTTP server.
    pub host: Ipv4Addr,
    /// Bind port for the HTTP server.
    pub port: u16,
    /// Enables verbose logging and debug endpoints.
    pub debug: bool,
    /// PostgreSQL connection string.
    pub database_url: String,
    /// Redis connection string.
    pub redis_url: String,
    /// Secret used to sign JWTs.
    pub jwt_secret: String,
}

impl AppConfig {
    /// Reads, parses and returns the environment variable `key` as `T`.
    fn parse_env<T>(key: &str) -> Result<T>
    where
        T: FromStr,
        T::Err: std::error::Error + Send + Sync + 'static,
    {
        env::var(key)
            .with_context(|| format!("missing environment variable: '{key}'"))?
            .parse()
            .with_context(|| format!("failed to parse variable '{key}'"))
    }

    /// Loads configuration from a `.env` file (if present) and environment.
    pub fn new() -> Result<Self> {
        let _ = dotenvy::dotenv();

        Ok(Self {
            host: Self::parse_env("host")?,
            port: Self::parse_env("port")?,
            debug: Self::parse_env("debug")?,
            database_url: Self::parse_env("database_url")?,
            redis_url: Self::parse_env("redis_url")?,
            jwt_secret: Self::parse_env("jwt_secret")?,
        })
    }
}

/// Shared application state injected into every HTTP handler.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Flattened set of walkable road coordinates.
    pub road_points: HashSet<i64>,
    /// The airport map matrix.
    pub map: MapMatrix,
    /// PostgreSQL connection pool.
    pub db_pool: Pool<ConnectionManager<PgConnection>>,
    /// Redis connection pool.
    pub redis_pool: Pool<Client>,
    /// JWT signing secret.
    pub jwt_secret: String,
}