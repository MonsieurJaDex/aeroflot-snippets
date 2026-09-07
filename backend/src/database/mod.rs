use std::time::Duration;

use anyhow::{Ok, Result, anyhow};
use diesel::{
    PgConnection,
    r2d2::{ConnectionManager, Pool},
};
use redis::Client;

pub mod schema;

pub async fn establish_pg_connection(
    url: &String,
) -> Result<Pool<ConnectionManager<PgConnection>>> {
    tracing::info!("Attempting to establish postgres connection...");

    let manager = ConnectionManager::<PgConnection>::new(url);
    let pool = Pool::builder()
        .test_on_check_out(true)
        .connection_timeout(Duration::from_secs(5))
        .build(manager)
        .map_err(|e| anyhow!(e.to_string()))?;

    Ok(pool)
}

pub async fn establish_redis_connection(url: &String) -> Result<Pool<Client>> {
    tracing::info!("Attempting to establish redis connection...");

    let client = Client::open(url.as_str())?;

    let pool = Pool::builder()
        .test_on_check_out(true)
        .connection_timeout(Duration::from_secs(5))
        .build(client)
        .map_err(|e| anyhow!(e.to_string()))?;

    Ok(pool)
}
