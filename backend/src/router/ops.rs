//! Request lifecycle: MCC intake, queue, engineer transitions, MEL, alerts, journal export.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{Response, StatusCode, header};
use axum::response::IntoResponse;
use axum::{Extension, Json};
use chrono::{Duration, Utc};
use diesel::prelude::*;
use redis::TypedCommands;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    ApiJson, active_map, bad_request, forbidden, internal_error, not_found, unprocessable,
};
use crate::database::schema::{defect_types, journal, tasks};
use crate::graph::AgentKind;
use crate::journal::JournalEvent;
use crate::middleware::AuthUser;
use crate::models::task::Task;
use crate::types::config::AppState;
use crate::types::dto::{
    AcceptCurrentTaskResponse, AllRequests, ErrorResponse, PlaneRequests, SelectionBasis,
};
use crate::types::enums::UserRole;
use crate::types::map::{Point, Route};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub text: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub plane_uuid: Option<String>,
    #[serde(default)]
    pub point: Option<Point>,
    #[serde(default)]
    pub ata_chapter: Option<String>,
    #[serde(default)]
    pub urgency: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestCreated {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReassignRequest {
    pub engineer_id: String,
    pub reason: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MelRequest {
    pub category: String,
    #[serde(default)]
    pub defer_until: Option<String>,
}

/// Body of `POST /api/tasks/{id}/timer`.
///
/// The organiser checklist asks for a way to shorten or lengthen a request's
/// clock, so an SLA breach or a comfortable margin can be staged on purpose.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskTimerRequest {
    /// Seconds added to the deadline: negative shortens the remaining time,
    /// positive lengthens it.
    pub delta_seconds: i64,
}

/// Answer of `POST /api/tasks/{id}/timer`.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskTimerResponse {
    pub request_id: String,
    /// New deadline, RFC3339.
    pub ends_at: String,
    /// Remaining seconds from now, negative when the deadline has passed.
    pub seconds_left: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AlertItem {
    pub request_id: String,
    pub message: String,
    pub alternative_engineer_id: Option<String>,
    pub alternative_margin_seconds: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AlertList {
    pub alerts: Vec<AlertItem>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewDefect {
    pub text: String,
    pub ata_chapter: String,
}

fn dispatcher(user: &AuthUser) -> Result<(), Response<axum::body::Body>> {
    if user.role == UserRole::Dispatcher {
        Ok(())
    } else {
        Err(forbidden("only dispatchers can manage the shift"))
    }
}

fn engineer(user: &AuthUser) -> Result<(), Response<axum::body::Body>> {
    if user.role == UserRole::Engineer {
        Ok(())
    } else {
        Err(forbidden("only engineers can update the job"))
    }
}

#[utoipa::path(
    post,
    path = "/api/requests",
    tag = "Tasks",
    operation_id = "createRequest",
    description = "MCC or crew files a remark. This does not assign an engineer.",
    security(("bearerAuth" = [])),
    request_body = CreateRequest,
    responses(
        (status = 200, description = "Remark recorded", body = RequestCreated),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 422, description = "Request body does not match the schema", body = ErrorResponse),
    ),
)]
pub async fn create_request(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<CreateRequest>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    if payload.text.trim().is_empty() {
        return Err(unprocessable("text is required"));
    }
    let active = active_map(&app_state)?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|_| internal_error())?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let (plane_id, point) = resolve_plane(&active, &mut redis_conn, &payload)?;
    let now = Utc::now();
    let task = Task {
        id: Uuid::new_v4(),
        description: payload.note.unwrap_or_else(|| payload.text.clone()),
        created_at: now,
        ends_at: now + Duration::seconds(active.location.sla_secs as i64),
        created_by: auth_user.id,
        assigned_engineer: None,
        issue_type: payload.text,
        plane_x: point.0,
        plane_y: point.1,
        plane_id,
        task_route: "[]".to_owned(),
        is_active: true,
        map_id: Some(active.id.clone()),
        status: "crew_remark".to_owned(),
        recorded_at: now,
        ata_chapter: payload.ata_chapter,
        defect_text: None,
        mel_category: None,
        mel_defer_until: None,
        waiting_part: false,
        tools_picked: false,
        arrived_at: None,
        source: Some(payload.source.unwrap_or_else(|| "mcc".to_owned())),
        urgency: payload.urgency.unwrap_or_else(|| "normal".to_owned()),
        eta_seconds: None,
        candidate_ms: None,
        selection_basis: None,
        sla_alerted: false,
        sla_breached: false,
    };
    diesel::insert_into(tasks::table)
        .values(&task)
        .execute(&mut pg_conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to store a request");
            internal_error()
        })?;
    crate::journal::record(
        &mut pg_conn,
        "request_recorded",
        Some(&auth_user.id.to_string()),
        Some(task.id),
        serde_json::json!({
            "source": task.source,
            "text": task.issue_type,
            "ata_chapter": task.ata_chapter,
            "urgency": task.urgency,
            "plane_id": task.plane_id,
        }),
        None,
    );
    Ok(Json(RequestCreated {
        id: task.id.to_string(),
        status: task.status,
    })
    .into_response())
}

fn resolve_plane(
    active: &crate::maps::ActiveMap,
    redis_conn: &mut redis::Connection,
    payload: &CreateRequest,
) -> Result<(Option<Uuid>, Point), Response<axum::body::Body>> {
    if let Some(point) = payload.point {
        if !active.map.in_bounds(point) {
            return Err(bad_request("point is outside of map bounds"));
        }
        let id = payload
            .plane_uuid
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| bad_request("invalid aircraft UUID"))?;
        return Ok((id, point));
    }
    let raw = payload
        .plane_uuid
        .as_deref()
        .ok_or_else(|| unprocessable("point or plane_uuid is required"))?;
    let id = Uuid::parse_str(raw).map_err(|_| bad_request("invalid aircraft UUID"))?;
    let key = active.vehicle_position_key(&crate::types::enums::VehicleType::Aircraft, id);
    let value: Option<String> = redis_conn.get(&key).map_err(|_| internal_error())?;
    let Some(value) = value else {
        return Err(not_found("aircraft was not found"));
    };
    let point = Point::from_value(value).map_err(|_| internal_error())?;
    Ok((Some(id), point))
}

#[utoipa::path(
    get,
    path = "/api/requests/all",
    tag = "Tasks",
    operation_id = "listAllRequests",
    description = "All request ids on the current map, grouped by aircraft. \
                   Requests without an aircraft are omitted.",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Grouped request ids", body = AllRequests),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Dispatcher role required", body = ErrorResponse),
    ),
)]
pub async fn list_all_requests(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    dispatcher(&auth_user)?;
    let active = active_map(&app_state)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;

    let rows = tasks::table
        .filter(tasks::map_id.eq(&active.id))
        .filter(tasks::plane_id.is_not_null())
        .order(tasks::created_at.asc())
        .select((tasks::plane_id.assume_not_null(), tasks::id))
        .load::<(Uuid, Uuid)>(&mut pg_conn)
        .map_err(|_| internal_error())?;

    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (plane_id, task_id) in rows {
        grouped
            .entry(plane_id.to_string())
            .or_default()
            .push(task_id.to_string());
    }

    Ok(Json(AllRequests(grouped)).into_response())
}

#[utoipa::path(
    get,
    path = "/api/requests/{plane_id}",
    tag = "Tasks",
    operation_id = "listRequestsForPlane",
    description = "All request ids registered against a specific aircraft on the current map.",
    security(("bearerAuth" = [])),
    params(("plane_id" = String, Path, description = "Aircraft id")),
    responses(
        (status = 200, description = "Request ids", body = PlaneRequests),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Dispatcher role required", body = ErrorResponse),
        (status = 404, description = "Aircraft was not found", body = ErrorResponse),
    ),
)]
pub async fn list_requests_for_plane(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Path(plane_id): Path<Uuid>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    dispatcher(&auth_user)?;
    let active = active_map(&app_state)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;

    let ids: Vec<Uuid> = tasks::table
        .filter(tasks::map_id.eq(&active.id))
        .filter(tasks::plane_id.eq(Some(plane_id)))
        .order(tasks::created_at.asc())
        .select(tasks::id)
        .load::<Uuid>(&mut pg_conn)
        .map_err(|_| internal_error())?;

    if ids.len() == 0 {
        return Ok(not_found("aircraft doesn't have any request"));
    }

    Ok(Json(PlaneRequests {
        plane_id: plane_id.to_string(),
        request_ids: ids.into_iter().map(|id| id.to_string()).collect(),
    })
    .into_response())
}

#[utoipa::path(
    post,
    path = "/api/requests/{id}/queue",
    tag = "Tasks",
    operation_id = "queueRequest",
    description = "Dispatcher accepts a crew remark into the unassigned queue.",
    security(("bearerAuth" = [])),
    params(("id" = String, Path, description = "Request id")),
    responses((status = 200, description = "Queued", body = RequestCreated)),
)]
pub async fn queue_request(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    dispatcher(&auth_user)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let task = load_task(&mut pg_conn, id)?;
    if task.status != "crew_remark" {
        return Err(bad_request("only a crew remark can be queued"));
    }
    diesel::update(tasks::table.find(id))
        .set(tasks::status.eq("unassigned"))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        "status_changed",
        Some(&auth_user.id.to_string()),
        Some(id),
        serde_json::json!({ "from": "crew_remark", "to": "unassigned" }),
        None,
    );
    Ok(Json(RequestCreated {
        id: id.to_string(),
        status: "unassigned".to_owned(),
    })
    .into_response())
}

#[utoipa::path(
    post,
    path = "/api/requests/{id}/reassign",
    tag = "Tasks",
    operation_id = "reassignRequest",
    description = "Shift lead moves a job in any non-terminal status to another engineer. \
                   The route and the ETA are rebuilt for the engineer who receives it.",
    security(("bearerAuth" = [])),
    request_body = ReassignRequest,
    params(("id" = String, Path, description = "Request id")),
    responses(
        (status = 200, description = "Reassigned", body = RequestCreated),
        (status = 400, description = "Terminal status, unknown or ineligible engineer",
         body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Dispatcher role required", body = ErrorResponse),
        (status = 404, description = "Request or engineer was not found", body = ErrorResponse),
    ),
)]
pub async fn reassign_request(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    ApiJson(payload): ApiJson<ReassignRequest>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    dispatcher(&auth_user)?;
    if payload.reason.trim().is_empty() {
        return Err(unprocessable("reason is required"));
    }
    let engineer_id =
        Uuid::parse_str(&payload.engineer_id).map_err(|_| bad_request("invalid engineer UUID"))?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|_| internal_error())?;
    let task = load_task(&mut pg_conn, id)?;
    if !is_reassignable(&task.status) {
        return Err(bad_request(format!(
            "a request in status {} cannot be reassigned",
            task.status
        )));
    }

    // The receiving engineer is ranked for the same aircraft: that is what
    // rebuilds the route and the ETA, so a reassignment never inherits the
    // travel time of the engineer it moved away from.
    let plan = super::build_candidates(&app_state, &mut pg_conn, &mut redis_conn, &task).await?;
    let row = plan
        .outcome
        .candidates
        .iter()
        .find(|row| row.engineer_id == engineer_id)
        .ok_or_else(|| not_found("engineer was not found"))?;
    if !row.eligible {
        return Err(bad_request(format!(
            "engineer cannot take this request: {}",
            row.reason
        )));
    }
    let travel = row.eta_seconds;
    let route_json = serde_json::to_string(&row.route).expect("route serializes");
    // ТЗ п.17: the basis stored with the task must describe the engineer who
    // now holds it, never the one it was moved away from.
    let basis_json = super::selection_basis_json(row, true).map_err(|e| {
        tracing::error!(error = %e, "failed to serialize the selection basis");
        internal_error()
    })?;
    diesel::update(tasks::table.find(id))
        .set((
            tasks::assigned_engineer.eq(Some(engineer_id)),
            tasks::status.eq("assigned"),
            tasks::task_route.eq(&route_json),
            tasks::eta_seconds.eq(Some(travel as i32)),
            tasks::candidate_ms.eq(Some(plan.outcome.elapsed_ms)),
            tasks::ata_chapter.eq(&plan.chapter),
            tasks::is_active.eq(true),
            tasks::waiting_part.eq(false),
            tasks::arrived_at.eq(None::<chrono::DateTime<Utc>>),
            tasks::selection_basis.eq(Some(basis_json)),
        ))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        "request_reassigned",
        Some(&auth_user.id.to_string()),
        Some(id),
        serde_json::json!({
            "from": task.assigned_engineer,
            "from_status": task.status,
            "to": engineer_id,
            "eta_seconds": travel,
            "reason": payload.reason,
        }),
        None,
    );
    Ok(Json(RequestCreated {
        id: id.to_string(),
        status: "assigned".to_owned(),
    })
    .into_response())
}

/// A task keeps its engineer until it is done; anything earlier may be moved
/// to somebody else or dropped back into the pool.
fn is_reassignable(status: &str) -> bool {
    !super::is_terminal_status(status)
}

#[utoipa::path(
    post,
    path = "/api/requests/{id}/unassign",
    tag = "Tasks",
    operation_id = "unassignRequest",
    description = "Shift lead takes the engineer off a job and puts it back into the pool.",
    security(("bearerAuth" = [])),
    params(("id" = String, Path, description = "Request id")),
    responses(
        (status = 200, description = "Back in the pool", body = RequestCreated),
        (status = 400, description = "Terminal status or the request has no engineer",
         body = ErrorResponse),
        (status = 401, description = "Missing or invalid access token", body = ErrorResponse),
        (status = 403, description = "Dispatcher role required", body = ErrorResponse),
        (status = 404, description = "Request was not found", body = ErrorResponse),
    ),
)]
pub async fn unassign_request(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    dispatcher(&auth_user)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let task = load_task(&mut pg_conn, id)?;
    if !is_reassignable(&task.status) {
        return Err(bad_request(format!(
            "a request in status {} cannot be unassigned",
            task.status
        )));
    }
    if task.status == "unassigned" {
        return Err(bad_request("request has no engineer to unassign"));
    }
    let previous = task.assigned_engineer;
    diesel::update(tasks::table.find(id))
        .set((
            tasks::assigned_engineer.eq(None::<Uuid>),
            tasks::status.eq("unassigned"),
            tasks::task_route.eq("[]"),
            tasks::eta_seconds.eq(None::<i32>),
            tasks::candidate_ms.eq(None::<i32>),
            tasks::waiting_part.eq(false),
            tasks::tools_picked.eq(false),
            tasks::arrived_at.eq(None::<chrono::DateTime<Utc>>),
            tasks::is_active.eq(true),
            // No engineer, no justification: a stale basis would be handed to
            // whoever accepts the request next.
            tasks::selection_basis.eq(None::<String>),
        ))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        "engineer_unassigned",
        Some(&auth_user.id.to_string()),
        Some(id),
        serde_json::json!({ "from": previous, "from_status": task.status }),
        None,
    );
    Ok(Json(RequestCreated {
        id: id.to_string(),
        status: "unassigned".to_owned(),
    })
    .into_response())
}

#[utoipa::path(
    post,
    path = "/api/tasks/current/accept",
    tag = "Tasks",
    operation_id = "acceptCurrentTask",
    security(("bearerAuth" = [])),
    responses((
        status = 200,
        description = "Accepted; `selection` carries the numeric justification of the pick",
        body = AcceptCurrentTaskResponse
    )),
)]
pub async fn accept_current(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    engineer(&auth_user)?;
    transition(
        &app_state,
        auth_user.id,
        "assigned",
        "in_progress",
        "status_changed",
    )
    .await
}

#[utoipa::path(
    post,
    path = "/api/tasks/current/reject",
    tag = "Tasks",
    operation_id = "rejectCurrentTask",
    security(("bearerAuth" = [])),
    responses((status = 200, description = "Rejected")),
)]
pub async fn reject_current(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    engineer(&auth_user)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let task = current_for(&mut pg_conn, auth_user.id)?;
    if task.status != "assigned" {
        return Err(bad_request("only an assigned request can be rejected"));
    }
    diesel::update(tasks::table.find(task.id))
        .set((tasks::status.eq("rejected"), tasks::is_active.eq(false)))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        "status_changed",
        Some(&auth_user.id.to_string()),
        Some(task.id),
        serde_json::json!({ "from": "assigned", "to": "rejected" }),
        None,
    );
    Ok(StatusCode::OK.into_response())
}

#[utoipa::path(
    post,
    path = "/api/tasks/current/tools",
    tag = "Tasks",
    operation_id = "pickTools",
    security(("bearerAuth" = [])),
    responses((status = 200, description = "Tools picked up")),
)]
pub async fn pick_tools(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    engineer(&auth_user)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let task = current_for(&mut pg_conn, auth_user.id)?;
    diesel::update(tasks::table.find(task.id))
        .set(tasks::tools_picked.eq(true))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        "status_changed",
        Some(&auth_user.id.to_string()),
        Some(task.id),
        serde_json::json!({ "tools_picked": true }),
        None,
    );
    Ok(StatusCode::OK.into_response())
}

#[utoipa::path(
    post,
    path = "/api/tasks/current/mel",
    tag = "Tasks",
    operation_id = "setMel",
    description = "Engineer sets the MEL category and optional deferral.",
    security(("bearerAuth" = [])),
    request_body = MelRequest,
    responses((status = 200, description = "MEL stored")),
)]
pub async fn set_mel(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<MelRequest>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    engineer(&auth_user)?;
    if !matches!(payload.category.as_str(), "A" | "B" | "C" | "D") {
        return Err(unprocessable("category must be A, B, C or D"));
    }
    let defer_until = payload
        .defer_until
        .as_deref()
        .map(|raw| {
            chrono::DateTime::parse_from_rfc3339(raw)
                .map(|time| time.with_timezone(&Utc))
                .map_err(|_| unprocessable("defer_until must be RFC3339"))
        })
        .transpose()?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let task = current_for(&mut pg_conn, auth_user.id)?;
    diesel::update(tasks::table.find(task.id))
        .set((
            tasks::mel_category.eq(Some(payload.category.clone())),
            tasks::mel_defer_until.eq(defer_until),
        ))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        "status_changed",
        Some(&auth_user.id.to_string()),
        Some(task.id),
        serde_json::json!({ "mel": payload.category, "defer_until": defer_until.map(|time| time.to_rfc3339()) }),
        None,
    );
    Ok(StatusCode::OK.into_response())
}

#[utoipa::path(
    post,
    path = "/api/tasks/current/waiting-part",
    tag = "Tasks",
    operation_id = "waitForPart",
    description = "Engineer waits for a part and stays attached to the aircraft.",
    security(("bearerAuth" = [])),
    responses((status = 200, description = "Waiting for a part")),
)]
pub async fn wait_for_part(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    engineer(&auth_user)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let task = current_for(&mut pg_conn, auth_user.id)?;
    diesel::update(tasks::table.find(task.id))
        // ТЗ п.19: waiting for a part sends the engineer back to the common
        // pool, so the aircraft no longer holds them and another request may
        // take them. `waiting_part` keeps the task open and counted as load.
        .set((
            tasks::status.eq("waiting_part"),
            tasks::waiting_part.eq(true),
            tasks::assigned_engineer.eq(None::<Uuid>),
        ))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        "status_changed",
        Some(&auth_user.id.to_string()),
        Some(task.id),
        serde_json::json!({ "from": task.status, "to": "waiting_part" }),
        None,
    );
    Ok(StatusCode::OK.into_response())
}

#[utoipa::path(
    post,
    path = "/api/tasks/{id}/route",
    tag = "Route",
    operation_id = "refreshTaskRoute",
    description = "Rebuilds the route from the engineer's latest position.",
    security(("bearerAuth" = [])),
    params(("id" = String, Path, description = "Request id")),
    responses((status = 200, description = "Route refreshed")),
)]
pub async fn refresh_route(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    let active = active_map(&app_state)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|_| internal_error())?;
    let task = load_task(&mut pg_conn, id)?;
    let Some(engineer_id) = task.assigned_engineer else {
        return Err(bad_request("request has no engineer"));
    };
    if auth_user.role == UserRole::Engineer && auth_user.id != engineer_id {
        return Err(forbidden("only engineers can update the job"));
    }
    let key = active.engineer_position_key(engineer_id);
    let raw: Option<String> = redis_conn.get(&key).map_err(|_| internal_error())?;
    let Some(raw) = raw else {
        return Err(not_found("engineer position is missing"));
    };
    let from = Point::from_value(raw).map_err(|_| internal_error())?;
    let planned = active
        .location
        .graph
        .route_stops(
            AgentKind::Engineer,
            &[from, Point::new(task.plane_x, task.plane_y)],
        )
        .map_err(|error| bad_request(error.to_string()))?;
    let route = Route::new(planned.points);
    let eta = planned.seconds.ceil() as i32;
    diesel::update(tasks::table.find(id))
        .set((
            tasks::task_route.eq(serde_json::to_string(&route).unwrap_or_else(|_| "[]".into())),
            tasks::eta_seconds.eq(Some(eta)),
        ))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        "route_recalculated",
        Some(&auth_user.id.to_string()),
        Some(id),
        serde_json::json!({ "eta_seconds": eta, "points": route.len() }),
        None,
    );
    Ok(Json(serde_json::json!({ "eta_seconds": eta, "route": route })).into_response())
}

#[utoipa::path(
    post,
    path = "/api/tasks/{id}/timer",
    tag = "Tasks",
    operation_id = "setTaskTimer",
    description = "Shifts the deadline of an open request: negative seconds shorten \
                   the remaining time, positive seconds lengthen it.",
    security(("bearerAuth" = [])),
    params(("id" = String, Path, description = "Request id")),
    request_body = TaskTimerRequest,
    responses(
        (status = 200, description = "Deadline shifted", body = TaskTimerResponse),
        (status = 400, description = "The request is closed or the shift is zero", body = ErrorResponse),
        (status = 403, description = "Authenticated user is not a dispatcher", body = ErrorResponse),
        (status = 404, description = "Request was not found", body = ErrorResponse),
    ),
)]
pub async fn set_task_timer(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    ApiJson(payload): ApiJson<TaskTimerRequest>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    if auth_user.role != UserRole::Dispatcher {
        return Err(forbidden("only dispatchers can change the request timer"));
    }
    if payload.delta_seconds == 0 {
        return Err(bad_request("delta_seconds must be non-zero"));
    }
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let task = load_task(&mut pg_conn, id)?;
    if !task.is_active {
        return Err(bad_request("request is closed"));
    }
    let ends_at = task.ends_at + Duration::seconds(payload.delta_seconds);
    diesel::update(tasks::table.find(id))
        .set(tasks::ends_at.eq(ends_at))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        "timer_changed",
        Some(&auth_user.id.to_string()),
        Some(id),
        serde_json::json!({
            "delta_seconds": payload.delta_seconds,
            "from": task.ends_at.to_rfc3339(),
            "to": ends_at.to_rfc3339(),
        }),
        None,
    );
    Ok(Json(TaskTimerResponse {
        request_id: id.to_string(),
        ends_at: ends_at.to_rfc3339(),
        seconds_left: (ends_at - Utc::now()).num_seconds(),
    })
    .into_response())
}

#[utoipa::path(
    get,
    path = "/api/alerts",
    tag = "Tasks",
    operation_id = "listAlerts",
    description = "Early SLA risk for the dispatcher, with an engineer who can still make it.",
    security(("bearerAuth" = [])),
    responses((status = 200, description = "Open alerts", body = AlertList)),
)]
pub async fn list_alerts(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    dispatcher(&auth_user)?;
    let active = active_map(&app_state)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|_| internal_error())?;
    let open = tasks::table
        .filter(tasks::is_active.eq(true))
        .filter(tasks::map_id.eq(&active.id))
        .filter(tasks::status.eq_any(["assigned", "in_progress"]))
        .select(Task::as_select())
        .load::<Task>(&mut pg_conn)
        .map_err(|_| internal_error())?;
    let now = Utc::now();
    let mut alerts = Vec::new();
    for task in open {
        if task.arrived_at.is_some() {
            continue;
        }
        let eta = task.eta_seconds.unwrap_or(0) as i64;
        let predicted = now + Duration::seconds(eta);
        let window = Duration::seconds(active.location.alert_before_secs as i64);
        if predicted <= task.ends_at || now + window < task.ends_at {
            continue;
        }
        let margin = (task.ends_at - predicted).num_seconds();
        if !task.sla_alerted {
            diesel::update(tasks::table.find(task.id))
                .set(tasks::sla_alerted.eq(true))
                .execute(&mut pg_conn)
                .ok();
            crate::journal::record(
                &mut pg_conn,
                "sla_risk",
                Some(&auth_user.id.to_string()),
                Some(task.id),
                serde_json::json!({ "eta_seconds": eta, "deadline": task.ends_at.to_rfc3339() }),
                None,
            );
        }
        if now > task.ends_at && !task.sla_breached {
            diesel::update(tasks::table.find(task.id))
                .set(tasks::sla_breached.eq(true))
                .execute(&mut pg_conn)
                .ok();
            crate::journal::record(
                &mut pg_conn,
                "sla_breached",
                Some(&auth_user.id.to_string()),
                Some(task.id),
                serde_json::json!({ "deadline": task.ends_at.to_rfc3339() }),
                None,
            );
        }
        // ТЗ п.21: a risk warning is only actionable when it names who could
        // still make it. Rank the pool for this very request and keep the best
        // engineer whose own travel time still lands before the deadline.
        let alternative = alternative_engineer(&app_state, &mut pg_conn, &mut redis_conn, &task)
            .await
            .and_then(|(id, eta)| {
                let left = (task.ends_at - (now + Duration::seconds(eta))).num_seconds();
                (left > 0).then_some((id, left))
            });
        let (alternative_engineer_id, alternative_margin_seconds) = match alternative {
            Some((id, left)) => (Some(id.to_string()), Some(left)),
            None => (None, Some(margin)),
        };
        let message = match &alternative_engineer_id {
            Some(id) => format!("не успевает, прогноз {eta} с; успевает инженер {id}"),
            None => format!("не успевает, прогноз {eta} с; замены в пуле нет"),
        };
        alerts.push(AlertItem {
            request_id: task.id.to_string(),
            message,
            alternative_engineer_id,
            alternative_margin_seconds,
        });
    }
    Ok(Json(AlertList { alerts }).into_response())
}

#[utoipa::path(
    post,
    path = "/api/defects",
    tag = "Classifier",
    operation_id = "createDefect",
    description = "Stores a new defect wording against an ATA chapter without a code change.",
    security(("bearerAuth" = [])),
    request_body = NewDefect,
    responses((status = 200, description = "Defect stored")),
)]
pub async fn create_defect(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    ApiJson(payload): ApiJson<NewDefect>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    dispatcher(&auth_user)?;
    let active = active_map(&app_state)?;
    if !active
        .location
        .ata
        .iter()
        .any(|item| item.chapter == payload.ata_chapter)
    {
        return Err(unprocessable("unknown ATA chapter"));
    }
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let id = Uuid::new_v4();
    diesel::insert_into(defect_types::table)
        .values((
            defect_types::id.eq(id),
            defect_types::body.eq(&payload.text),
            defect_types::ata_chapter.eq(&payload.ata_chapter),
            defect_types::created_at.eq(Utc::now()),
        ))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        "defect_added",
        Some(&auth_user.id.to_string()),
        None,
        serde_json::json!({ "text": payload.text, "ata_chapter": payload.ata_chapter }),
        None,
    );
    Ok(Json(serde_json::json!({ "id": id })).into_response())
}

#[utoipa::path(
    get,
    path = "/api/journal.csv",
    tag = "Tasks",
    operation_id = "exportJournal",
    description = "Immutable chronological journal, one event per line.",
    security(("bearerAuth" = [])),
    responses((status = 200, description = "CSV journal")),
)]
pub async fn export_journal(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    dispatcher(&auth_user)?;
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let rows = journal::table
        .order(journal::at.asc())
        .select(JournalEvent::as_select())
        .load::<JournalEvent>(&mut pg_conn)
        .map_err(|_| internal_error())?;
    let mut csv = String::from("at,kind,actor,task_id,duration_ms,params\n");
    for row in rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{}\n",
            csv_field(&row.at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            csv_field(&row.kind),
            csv_field(&row.actor.unwrap_or_default()),
            csv_field(&row.task_id.map(|id| id.to_string()).unwrap_or_default()),
            csv_field(&row.duration_ms.map(|ms| ms.to_string()).unwrap_or_default()),
            csv_field(&row.params.replace('\n', " "))
        ));
    }
    Ok(([(header::CONTENT_TYPE, "text/csv; charset=utf-8")], csv).into_response())
}

/// Quotes a CSV field per RFC 4180: `params` holds JSON, so a bare value with
/// commas would split one event across several columns.
fn csv_field(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

async fn transition(
    app_state: &AppState,
    engineer_id: Uuid,
    from: &str,
    to: &str,
    kind: &str,
) -> Result<Response<axum::body::Body>, Response<axum::body::Body>> {
    let mut pg_conn = app_state.db_pool.get().map_err(|_| internal_error())?;
    let task = current_for(&mut pg_conn, engineer_id)?;
    if task.status != from {
        return Err(bad_request(format!(
            "request status is {0}, expected {from}",
            task.status
        )));
    }
    diesel::update(tasks::table.find(task.id))
        .set(tasks::status.eq(to))
        .execute(&mut pg_conn)
        .map_err(|_| internal_error())?;
    crate::journal::record(
        &mut pg_conn,
        kind,
        Some(&engineer_id.to_string()),
        Some(task.id),
        serde_json::json!({ "from": from, "to": to }),
        None,
    );
    // The pick's justification is stored as JSON by `assign_engineer`. A task
    // assigned before the column existed, or a row we cannot read, still
    // accepts: the caller just gets no numbers instead of an error.
    let selection = task
        .selection_basis
        .as_deref()
        .and_then(|raw| serde_json::from_str::<SelectionBasis>(raw).ok());
    Ok(Json(AcceptCurrentTaskResponse {
        status: to.to_owned(),
        selection,
    })
    .into_response())
}

fn load_task(
    conn: &mut diesel::PgConnection,
    id: Uuid,
) -> Result<Task, Response<axum::body::Body>> {
    tasks::table
        .find(id)
        .select(Task::as_select())
        .first::<Task>(conn)
        .optional()
        .map_err(|_| internal_error())?
        .ok_or_else(|| not_found("request was not found"))
}

fn current_for(
    conn: &mut diesel::PgConnection,
    engineer_id: Uuid,
) -> Result<Task, Response<axum::body::Body>> {
    tasks::table
        .filter(tasks::assigned_engineer.eq(engineer_id))
        .filter(tasks::is_active.eq(true))
        .order(tasks::created_at.desc())
        .select(Task::as_select())
        .first::<Task>(conn)
        .optional()
        .map_err(|_| internal_error())?
        .ok_or_else(|| not_found("engineer has no active task"))
}

/// Best engineer from the pool who is not the one already held by `task`, with
/// their own travel time. Used by [`list_alerts`] to name a real alternative
/// (ТЗ п.21) instead of an empty id; a ranking that cannot be built yields
/// `None`, and the alert still goes out.
async fn alternative_engineer(
    app_state: &Arc<AppState>,
    pg_conn: &mut diesel::PgConnection,
    redis_conn: &mut redis::Connection,
    task: &Task,
) -> Option<(Uuid, i64)> {
    let plan = super::build_candidates(app_state, pg_conn, redis_conn, task)
        .await
        .ok()?;
    plan.outcome
        .candidates
        .into_iter()
        .filter(|row| row.eligible && Some(row.engineer_id) != task.assigned_engineer)
        .min_by(|left, right| {
            left.score
                .partial_cmp(&right.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|row| (row.engineer_id, row.eta_seconds as i64))
}
