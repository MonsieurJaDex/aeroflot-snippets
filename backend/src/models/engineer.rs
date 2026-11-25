use diesel::{Selectable, deserialize::Queryable, prelude::Insertable};
use uuid::Uuid;
use validator::Validate;

use crate::types::enums::EngineerType;

// Business and ORM model for engineers
#[derive(Debug, Queryable, Selectable, Insertable, Validate)]
#[diesel(table_name = crate::database::schema::engineers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Engineer {
    pub id: Uuid,

    #[validate(email)]
    pub email: String,

    pub name: String,
    pub password_hash: String,
    pub engineer_type: EngineerType,

    /// Organiser roster id (`AT-0001` …) whose licence this account holds.
    /// Absent means the account is gated by `engineer_type` alone.
    pub tabel_id: Option<String>,
}
