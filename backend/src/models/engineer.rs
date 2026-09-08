use diesel::{Selectable, deserialize::Queryable};
use uuid::Uuid;
use validator::Validate;

use crate::types::enums::EngineerType;

#[derive(Debug, Queryable, Selectable, Validate)]
#[diesel(table_name = crate::database::schema::engineers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Engineer {
    pub id: Uuid,

    #[validate(email)]
    pub email: String,

    pub name: String,
    pub password_hash: String,
    pub engineer_type: EngineerType,
}

impl Engineer {
    // fn get_position(redis: Pool<>) -> Point {}
}
