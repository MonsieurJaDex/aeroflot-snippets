use std::time::Duration;

use anyhow::{Ok, Result, anyhow};
use diesel::{
    PgConnection,
    r2d2::{ConnectionManager, Pool},
};

pub mod schema;

pub fn establish_connection(url: &String) -> Result<Pool<ConnectionManager<PgConnection>>> {
    let manager = ConnectionManager::<PgConnection>::new(url);
    let pool = Pool::builder()
        .test_on_check_out(true)
        .connection_timeout(Duration::from_secs(5))
        .build(manager)
        .map_err(|e| anyhow!(e.to_string()))?;

    Ok(pool)
}
