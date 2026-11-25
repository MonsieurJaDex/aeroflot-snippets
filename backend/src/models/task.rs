use chrono::{DateTime, Utc};
use diesel::{Selectable, deserialize::Queryable, prelude::Insertable};
use uuid::Uuid;

// business and diesel model for tasks
#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::database::schema::tasks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Task {
    pub id: Uuid,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub assigned_engineer: Option<Uuid>,
    pub issue_type: String,
    pub plane_x: i64,
    pub plane_y: i64,
    pub plane_id: Option<Uuid>,
    pub task_route: String,
    pub is_active: bool,
    /// Map the route was built on. `None` for tasks created before map selection.
    pub map_id: Option<String>,
    pub status: String,
    pub recorded_at: DateTime<Utc>,
    pub ata_chapter: Option<String>,
    pub defect_text: Option<String>,
    pub mel_category: Option<String>,
    pub mel_defer_until: Option<DateTime<Utc>>,
    pub waiting_part: bool,
    pub tools_picked: bool,
    pub arrived_at: Option<DateTime<Utc>>,
    pub source: Option<String>,
    pub urgency: String,
    pub eta_seconds: Option<i32>,
    pub candidate_ms: Option<i32>,
    /// JSON of [`crate::types::dto::SelectionBasis`]: why this engineer was
    /// chosen. Written once at assignment, read back when the engineer accepts.
    pub selection_basis: Option<String>,
    pub sla_alerted: bool,
    pub sla_breached: bool,
}
