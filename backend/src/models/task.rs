use chrono::{DateTime, Utc};
use diesel::{Selectable, deserialize::Queryable};
use uuid::Uuid;

use crate::types::enums::AircraftIssue;

// diesel model
#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::database::schema::tasks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Task {
    pub id: Uuid,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub assigned_engineer: Uuid,
    pub issue_type: AircraftIssue,
    pub is_active: bool,
}
