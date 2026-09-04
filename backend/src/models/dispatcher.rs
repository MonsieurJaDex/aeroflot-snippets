use diesel::{Selectable, deserialize::Queryable};
use uuid::Uuid;
use validator::Validate;

#[derive(Validate)]
pub struct Dispatcher {
    pub id: Uuid,
    pub name: String,

    #[validate(email)]
    pub email: String,

    pub password_hash: String,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::database::schema::dispatchers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DispatcherRow {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub password_hash: String,
}
