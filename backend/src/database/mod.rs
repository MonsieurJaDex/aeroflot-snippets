use anyhow::{Result, anyhow};
use diesel::{Connection, PgConnection};

pub mod schema;

pub fn establish_connection(url: &String) -> Result<PgConnection> {
    PgConnection::establish(url.as_str()).map_err(|e| anyhow!(e.to_string()))
}
