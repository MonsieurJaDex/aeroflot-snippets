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
    use diesel::sql_types::*;

    aircraft_movements (id) {
        id -> Uuid,
        aircraft_id -> Uuid,
        map_id -> Text,
        plane_x -> Int8,
        plane_y -> Int8,
        arrived_at -> Timestamptz,
        departed_at -> Nullable<Timestamptz>,
    }
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
        name -> Varchar,
        engineer_type -> EngineerType,
        password_hash -> Varchar,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    tasks (id) {
        id -> Uuid,
        description -> Text,
        created_at -> Timestamptz,
        ends_at -> Timestamptz,
        created_by -> Uuid,
        assigned_engineer -> Nullable<Uuid>,
        issue_type -> Text,
        plane_x -> Int8,
        plane_y -> Int8,
        plane_id -> Nullable<Uuid>,
        task_route -> Text,
        is_accepted -> Bool,
        is_active -> Bool,
        map_id -> Nullable<Text>,
        status -> Text,
        recorded_at -> Timestamptz,
        ata_chapter -> Nullable<Text>,
        defect_text -> Nullable<Text>,
        mel_category -> Nullable<Text>,
        mel_defer_until -> Nullable<Timestamptz>,
        waiting_part -> Bool,
        tools_picked -> Bool,
        arrived_at -> Nullable<Timestamptz>,
        source -> Nullable<Text>,
        urgency -> Text,
        eta_seconds -> Nullable<Integer>,
        candidate_ms -> Nullable<Integer>,
        sla_alerted -> Bool,
        sla_breached -> Bool,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    journal (id) {
        id -> Uuid,
        at -> Timestamptz,
        kind -> Text,
        actor -> Nullable<Text>,
        task_id -> Nullable<Uuid>,
        params -> Text,
        duration_ms -> Nullable<Integer>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    defect_types (id) {
        id -> Uuid,
        body -> Text,
        ata_chapter -> Text,
        created_at -> Timestamptz,
    }
}

diesel::joinable!(tasks -> dispatchers (created_by));
diesel::joinable!(tasks -> engineers (assigned_engineer));

diesel::allow_tables_to_appear_in_same_query!(
    aircraft_movements,
    defect_types,
    dispatchers,
    engineers,
    journal,
    tasks,
);
