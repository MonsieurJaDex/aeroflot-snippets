// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "aircraft_issue"))]
    pub struct AircraftIssue;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "engineer_type"))]
    pub struct EngineerType;
}

diesel::table! {
    dispatchers (id) {
        id -> Uuid,
        name -> Varchar,
        email -> Varchar,
        password_hash -> Varchar,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::EngineerType;

    engineers (id) {
        id -> Uuid,
        email -> Varchar,
        engineer_type -> EngineerType,
        password_hash -> Varchar,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::AircraftIssue;

    tasks (id) {
        id -> Uuid,
        created_at -> Timestamptz,
        ends_at -> Timestamptz,
        created_by -> Uuid,
        assigned_engineer -> Uuid,
        issue_type -> AircraftIssue,
        is_active -> Bool,
    }
}

diesel::joinable!(tasks -> dispatchers (created_by));
diesel::joinable!(tasks -> engineers (assigned_engineer));

diesel::allow_tables_to_appear_in_same_query!(dispatchers, engineers, tasks,);
