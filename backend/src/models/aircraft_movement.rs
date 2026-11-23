use chrono::{DateTime, Utc};
use diesel::{Selectable, deserialize::Queryable, prelude::Insertable};
use uuid::Uuid;

/// One stay of an aircraft on a map, from arrival until departure.
#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::database::schema::aircraft_movements)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct AircraftMovement {
    pub id: Uuid,
    pub aircraft_id: Uuid,
    pub map_id: String,
    pub plane_x: i64,
    pub plane_y: i64,
    pub arrived_at: DateTime<Utc>,
    pub departed_at: Option<DateTime<Utc>>,
}
