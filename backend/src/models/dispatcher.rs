use diesel::{Selectable, deserialize::Queryable};
use uuid::Uuid;
use validator::Validate;

#[derive(Validate, Queryable, Selectable)]
#[diesel(table_name = crate::database::schema::dispatchers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Dispatcher {
    pub id: Uuid,
    pub name: String,

    #[validate(email)]
    pub email: String,

    pub password_hash: String,
}
