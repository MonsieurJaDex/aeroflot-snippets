//! Append-only event journal. Rows are inserted and never updated.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::database::schema::journal;

#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = journal)]
pub struct JournalEvent {
    pub id: Uuid,
    pub at: DateTime<Utc>,
    pub kind: String,
    pub actor: Option<String>,
    pub task_id: Option<Uuid>,
    pub params: String,
    pub duration_ms: Option<i32>,
}

pub fn record(
    conn: &mut diesel::PgConnection,
    kind: &str,
    actor: Option<&str>,
    task_id: Option<Uuid>,
    params: serde_json::Value,
    duration_ms: Option<i32>,
) {
    let event = JournalEvent {
        id: Uuid::new_v4(),
        at: Utc::now(),
        kind: kind.to_owned(),
        actor: actor.map(str::to_owned),
        task_id,
        params: params.to_string(),
        duration_ms,
    };
    if let Err(error) = diesel::insert_into(journal::table)
        .values(&event)
        .execute(conn)
    {
        tracing::error!(%kind, %error, "journal append failed");
    }
}
