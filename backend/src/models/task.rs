use chrono::{DateTime, Utc};
use diesel::{Selectable, deserialize::Queryable};
use uuid::Uuid;

use crate::{
    models::{dispatcher::Dispatcher, engineer::Engineer},
    types::enums::AircraftIssue,
};

// доменная модель (то, что видит бизнес-логика)
pub struct Task {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub created_by: Dispatcher,      // полный объект
    pub assigned_engineer: Engineer, // полный объект
    pub issue_type: AircraftIssue,
    pub is_active: bool,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::database::schema::tasks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TaskRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub assigned_engineer: Uuid,
    pub issue_type: AircraftIssue,
    pub is_active: bool,
}
