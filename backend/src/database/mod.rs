use anyhow::{Result, anyhow};
use diesel::{
    PgConnection,
    r2d2::{ConnectionManager, Pool},
};

pub mod schema;

pub fn establish_connection(url: &String) -> Result<Pool<ConnectionManager<PgConnection>>> {
    let manager = ConnectionManager::<PgConnection>::new(url);
    Pool::builder()
        .test_on_check_out(true)
        .build(manager)
        .map_err(|e| anyhow!(e.to_string()))
}
