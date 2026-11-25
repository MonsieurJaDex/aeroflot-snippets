//! Request lifecycle endpoints: intake, queue, reassignment and engineer actions.

use axum::http::StatusCode;
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::database::schema::tasks;
use crate::tests::support::{expect_status, TestContext};
use crate::types::dto::SelectionBasis;
use crate::types::enums::EngineerType;
use crate::types::map::Point;

#[tokio::test]
async fn creates_request_and_lists_it_by_aircraft() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let plane_id = Uuid::new_v4();

    let created = expect_status(
        ctx.post(
            "/api/requests",
            &dispatcher.token,
            json!({
                "text": "подтекание топлива из дренажных колпачков",
                "note": "Обнаружены следы подтекания",
                "plane_uuid": plane_id,
                "point": [10, 10],
                "ata_chapter": "28",
                "urgency": "normal",
                "source": "mcc"
            }),
        )
        .await,
        StatusCode::OK,
    );
    let request_id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(created["status"], "crew_remark");

    let task = ctx.task(request_id);
    assert_eq!(task.status, "crew_remark");
    assert_eq!(task.issue_type, "подтекание топлива из дренажных колпачков");
    assert_eq!(task.description, "Обнаружены следы подтекания");
    assert_eq!(task.plane_id, Some(plane_id));
    assert_eq!(task.map_id.as_deref(), Some(ctx.active_id().as_str()));
    assert_eq!(task.created_by, dispatcher.id);
    assert_eq!((task.plane_x, task.plane_y), (10, 10));
    assert!(ctx.journal_count() >= 1);

    let all = expect_status(
        ctx.get("/api/requests/all", &dispatcher.token).await,
        StatusCode::OK,
    );
    assert_eq!(
        all[plane_id.to_string()],
        json!([request_id.to_string()]),
        "request should be grouped under its aircraft"
    );

    let by_plane = expect_status(
        ctx.get(&format!("/api/requests/{plane_id}"), &dispatcher.token)
            .await,
        StatusCode::OK,
    );
    assert_eq!(by_plane["plane_id"], plane_id.to_string());
    assert_eq!(by_plane["request_ids"], json!([request_id.to_string()]));

    expect_status(
        ctx.get(
            &format!("/api/requests/{}", Uuid::new_v4()),
            &dispatcher.token,
        )
        .await,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn validates_request_intake() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();

    expect_status(
        ctx.post(
            "/api/requests",
            &dispatcher.token,
            json!({ "text": "   ", "point": [1, 1] }),
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
    expect_status(
        ctx.post(
            "/api/requests",
            &dispatcher.token,
            json!({ "text": "no position" }),
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
    expect_status(
        ctx.post(
            "/api/requests",
            &dispatcher.token,
            json!({ "text": "outside", "point": [999, 999] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    expect_status(
        ctx.get("/api/requests/all", &engineer.token).await,
        StatusCode::FORBIDDEN,
    );
}

#[tokio::test]
async fn queues_crew_remark_once() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();

    let created = expect_status(
        ctx.post(
            "/api/requests",
            &dispatcher.token,
            json!({ "text": "scuff on fairing", "point": [12, 12] }),
        )
        .await,
        StatusCode::OK,
    );
    let request_id: Uuid = created["id"].as_str().unwrap().parse().unwrap();

    let queued = expect_status(
        ctx.post(
            &format!("/api/requests/{request_id}/queue"),
            &dispatcher.token,
            json!(null),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(queued["status"], "unassigned");
    assert_eq!(ctx.task(request_id).status, "unassigned");

    expect_status(
        ctx.post(
            &format!("/api/requests/{request_id}/queue"),
            &dispatcher.token,
            json!(null),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.post(
            &format!("/api/requests/{}/queue", Uuid::new_v4()),
            &dispatcher.token,
            json!(null),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn reassigns_an_in_progress_request() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let first = ctx.engineer(EngineerType::AvionicsEngineer);
    // `seeded issue` classifies as a chapter 28 task, so only a fueling crew or
    // an engine technician is admitted; the receiving engineer must also have a
    // position, or no route can be built for them.
    let second = ctx.engineer(EngineerType::EngineTechnician);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), first.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), second.id),
        "5,30",
    );
    let request_id = ctx.seed_task(
        dispatcher.id,
        Some(first.id),
        "in_progress",
        None,
        Point::new(10, 10),
    );

    let body = expect_status(
        ctx.post(
            &format!("/api/requests/{request_id}/reassign"),
            &dispatcher.token,
            json!({ "engineer_id": second.id, "reason": "shift change" }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(body["status"], "assigned");

    let task = ctx.task(request_id);
    assert_eq!(task.assigned_engineer, Some(second.id));
    assert_eq!(task.status, "assigned");
    assert_ne!(task.task_route, "[]");
    assert!(
        task.eta_seconds.expect("eta is rebuilt") > 0,
        "the ETA must be rebuilt for the engineer who receives the job"
    );

    expect_status(
        ctx.post(
            &format!("/api/requests/{request_id}/reassign"),
            &dispatcher.token,
            json!({ "engineer_id": "bad-uuid", "reason": "reason" }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    // A job in `assigned` is still the dispatcher's to move.
    let assigned = ctx.seed_task(
        dispatcher.id,
        Some(first.id),
        "assigned",
        None,
        Point::new(1, 1),
    );
    expect_status(
        ctx.post(
            &format!("/api/requests/{assigned}/reassign"),
            &dispatcher.token,
            json!({ "engineer_id": second.id, "reason": "before the walk" }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(ctx.task(assigned).assigned_engineer, Some(second.id));

    // A finished job is terminal for both reassign and unassign.
    let done = ctx.seed_task(
        dispatcher.id,
        Some(first.id),
        "done",
        None,
        Point::new(1, 1),
    );
    expect_status(
        ctx.post(
            &format!("/api/requests/{done}/reassign"),
            &dispatcher.token,
            json!({ "engineer_id": second.id, "reason": "too late" }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.post(
            &format!("/api/requests/{done}/unassign"),
            &dispatcher.token,
            json!(null),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
}

#[tokio::test]
async fn engineer_moves_current_task_through_its_states() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::CrewRemarksHandler);
    let plane_id = Uuid::new_v4();
    let request_id = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "assigned",
        Some(plane_id),
        Point::new(10, 10),
    );

    let current = expect_status(
        ctx.get("/api/tasks/current", &engineer.token).await,
        StatusCode::OK,
    );
    assert_eq!(current["id"], request_id.to_string());
    assert_eq!(current["status"], "assigned");
    assert_eq!(current["plane_uuid"], plane_id.to_string());
    assert_eq!(current["arrived"], false);

    expect_status(
        ctx.post("/api/tasks/current/accept", &engineer.token, json!(null))
            .await,
        StatusCode::OK,
    );
    assert_eq!(ctx.task(request_id).status, "in_progress");

    expect_status(
        ctx.post("/api/tasks/current/tools", &engineer.token, json!(null))
            .await,
        StatusCode::OK,
    );
    assert!(ctx.task(request_id).tools_picked);

    expect_status(
        ctx.post(
            "/api/tasks/current/mel",
            &engineer.token,
            json!({ "category": "B", "defer_until": "2026-12-01T00:00:00Z" }),
        )
        .await,
        StatusCode::OK,
    );
    let task = ctx.task(request_id);
    assert_eq!(task.mel_category.as_deref(), Some("B"));
    assert!(task.mel_defer_until.is_some());

    expect_status(
        ctx.post(
            "/api/tasks/current/mel",
            &engineer.token,
            json!({ "category": "Z" }),
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );

    expect_status(
        ctx.post(
            "/api/tasks/current/waiting-part",
            &engineer.token,
            json!(null),
        )
        .await,
        StatusCode::OK,
    );
    let task = ctx.task(request_id);
    assert_eq!(task.status, "waiting_part");
    assert!(task.waiting_part);
    // ТЗ п.19: waiting for a part returns the engineer to the common pool.
    assert_eq!(task.assigned_engineer, None);
}

#[tokio::test]
async fn reject_complete_and_route_refresh_write_to_database() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);

    let rejected = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "assigned",
        None,
        Point::new(10, 10),
    );
    expect_status(
        ctx.post("/api/tasks/current/reject", &engineer.token, json!(null))
            .await,
        StatusCode::OK,
    );
    let task = ctx.task(rejected);
    assert_eq!(task.status, "rejected");
    assert!(!task.is_active);

    let completed = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "in_progress",
        None,
        Point::new(10, 10),
    );
    expect_status(
        ctx.post("/api/tasks/current/complete", &engineer.token, json!(null))
            .await,
        StatusCode::OK,
    );
    let task = ctx.task(completed);
    assert_eq!(task.status, "done");
    assert!(!task.is_active);

    let routed = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "assigned",
        None,
        Point::new(10, 10),
    );
    let position_key = format!("map:{}:position:{}", ctx.active_id(), engineer.id);
    ctx.redis_set(&position_key, "5,5");
    let body = expect_status(
        ctx.post(
            &format!("/api/tasks/{routed}/route"),
            &engineer.token,
            json!(null),
        )
        .await,
        StatusCode::OK,
    );
    assert!(body["eta_seconds"].as_i64().unwrap_or(0) > 0);
    assert!(body["route"].as_array().unwrap().len() > 1);

    let task = ctx.task(routed);
    assert_ne!(task.task_route, "[]");
    assert!(task.eta_seconds.is_some());

    let unrouted = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "assigned",
        None,
        Point::new(2, 2),
    );
    ctx.redis_del(&position_key);
    expect_status(
        ctx.post(
            &format!("/api/tasks/{unrouted}/route"),
            &engineer.token,
            json!(null),
        )
        .await,
        StatusCode::NOT_FOUND,
    );

    let pending = ctx.seed_task(dispatcher.id, None, "unassigned", None, Point::new(2, 2));
    expect_status(
        ctx.post(
            &format!("/api/tasks/{pending}/route"),
            &engineer.token,
            json!(null),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
}

#[tokio::test]
async fn current_task_detects_engineer_arrival_in_database() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let request_id = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "in_progress",
        None,
        Point::new(10, 10),
    );
    let position_key = format!("map:{}:position:{}", ctx.active_id(), engineer.id);
    ctx.redis_set(&position_key, "10,10");

    expect_status(
        ctx.get("/api/tasks/current", &engineer.token).await,
        StatusCode::OK,
    );
    assert!(
        ctx.task(request_id).arrived_at.is_some(),
        "arrival should be persisted"
    );

    // The first response is built from the row loaded before the arrival update,
    // so the persisted flag is visible on the next read.
    let body = expect_status(
        ctx.get("/api/tasks/current", &engineer.token).await,
        StatusCode::OK,
    );
    assert_eq!(body["arrived"], true);

    let empty = ctx.dispatcher();
    expect_status(
        ctx.get("/api/tasks/current", &empty.token).await,
        StatusCode::FORBIDDEN,
    );
    let idle = ctx.engineer(EngineerType::FuelingCrew);
    expect_status(
        ctx.get("/api/tasks/current", &idle.token).await,
        StatusCode::NOT_FOUND,
    );
    expect_status(
        ctx.post("/api/tasks/current/accept", &empty.token, json!(null))
            .await,
        StatusCode::FORBIDDEN,
    );
}

#[tokio::test]
async fn alerts_flag_sla_risk_and_breach_in_database() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let now = chrono::Utc::now();

    let risky = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "assigned",
        None,
        Point::new(10, 10),
    );
    diesel::update(tasks::table.find(risky))
        .set((
            tasks::ends_at.eq(now + chrono::Duration::seconds(100)),
            tasks::eta_seconds.eq(Some(200)),
        ))
        .execute(&mut ctx.db())
        .unwrap();

    let breached = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "in_progress",
        None,
        Point::new(10, 10),
    );
    diesel::update(tasks::table.find(breached))
        .set((
            tasks::ends_at.eq(now - chrono::Duration::seconds(60)),
            tasks::eta_seconds.eq(Some(0)),
        ))
        .execute(&mut ctx.db())
        .unwrap();

    let body = expect_status(
        ctx.get("/api/alerts", &dispatcher.token).await,
        StatusCode::OK,
    );
    let alerts = body["alerts"].as_array().unwrap();
    assert_eq!(alerts.len(), 2);
    assert!(alerts
        .iter()
        .any(|alert| alert["request_id"] == risky.to_string()));

    assert!(ctx.task(risky).sla_alerted);
    assert!(ctx.task(breached).sla_alerted);
    assert!(ctx.task(breached).sla_breached);
    assert!(ctx.journal_count() >= 2);

    expect_status(
        ctx.get("/api/alerts", &engineer.token).await,
        StatusCode::FORBIDDEN,
    );
}

#[tokio::test]
async fn stores_defects_and_exports_the_journal() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::EngineTechnician);

    let body = expect_status(
        ctx.post(
            "/api/defects",
            &dispatcher.token,
            json!({ "text": "трещина лопатки компрессора", "ata_chapter": "72" }),
        )
        .await,
        StatusCode::OK,
    );
    let defect_id: Uuid = body["id"].as_str().unwrap().parse().unwrap();
    let row: (String, String) = crate::database::schema::defect_types::table
        .find(defect_id)
        .select((
            crate::database::schema::defect_types::body,
            crate::database::schema::defect_types::ata_chapter,
        ))
        .first(&mut ctx.db())
        .expect("defect row should exist");
    assert_eq!(row.0, "трещина лопатки компрессора");
    assert_eq!(row.1, "72");
    assert_eq!(ctx.defect_count(), 1);

    expect_status(
        ctx.post(
            "/api/defects",
            &dispatcher.token,
            json!({ "text": "unknown chapter", "ata_chapter": "99" }),
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
    expect_status(
        ctx.post(
            "/api/defects",
            &engineer.token,
            json!({ "text": "not allowed", "ata_chapter": "72" }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );

    let (status, csv) = ctx
        .raw_request(
            axum::http::Method::GET,
            "/api/journal.csv",
            Some(&dispatcher.token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(csv.starts_with("at,kind,actor,task_id,duration_ms,params"));
    assert!(
        csv.contains("defect_added"),
        "journal should contain the event"
    );
    // `params` is JSON, so its commas must stay inside one quoted column: split
    // on commas only outside quoted fields, where a doubled `""` is a literal.
    for line in csv.lines().skip(1) {
        let fields = csv_fields(line);
        assert_eq!(fields.len(), 6, "journal row should have 6 columns: {line}");
        serde_json::from_str::<serde_json::Value>(&fields[5])
            .expect("params should stay valid JSON");
    }
    let defect_row = csv
        .lines()
        .find(|line| line.contains("defect_added"))
        .expect("defect_added row");
    let params = csv_fields(defect_row).pop().expect("params column");
    let params: serde_json::Value =
        serde_json::from_str(&params).expect("params should stay valid JSON");
    assert!(
        params["ata_chapter"].as_str().is_some(),
        "params must keep their comma-separated shape: {params}"
    );

    expect_status(
        ctx.get("/api/journal.csv", &engineer.token).await,
        StatusCode::FORBIDDEN,
    );
}

/// Splits one RFC 4180 row: commas separate fields only outside quotes, and a
/// doubled `""` inside a quoted field is a literal quote.
fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => fields.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    assert!(!in_quotes, "unbalanced quotes in journal row: {line}");
    fields.push(current);
    fields
}

/// Files and queues a request through the public endpoints. Duplicated from
/// `tests::assign` so this file does not depend on another module's helper.
async fn create_queued_request(
    ctx: &TestContext,
    dispatcher: &str,
    plane_id: Uuid,
    point: Point,
) -> Uuid {
    let created = expect_status(
        ctx.post(
            "/api/requests",
            dispatcher,
            json!({
                "text": "уход гироплатформы ИНС, курс смещается",
                "plane_uuid": plane_id,
                "point": [point.0, point.1],
                "ata_chapter": "30"
            }),
        )
        .await,
        StatusCode::OK,
    );
    let request_id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    expect_status(
        ctx.post(
            &format!("/api/requests/{request_id}/queue"),
            dispatcher,
            json!(null),
        )
        .await,
        StatusCode::OK,
    );
    request_id
}

fn candidate_for<'a>(assigned: &'a serde_json::Value, engineer_id: Uuid) -> &'a serde_json::Value {
    assigned["candidates"]
        .as_array()
        .expect("candidates array")
        .iter()
        .find(|row| row["engineer_id"] == engineer_id.to_string())
        .expect("engineer is ranked")
}

/// ТЗ п.19: waiting for a part sends the engineer back to the common pool. The
/// task stays open and keeps counting as load, but the aircraft no longer holds
/// the engineer, so another request may take them.
#[tokio::test]
async fn waiting_part_returns_engineer_to_pool() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let waiting = ctx.engineer(EngineerType::AvionicsEngineer);
    let free = ctx.engineer(EngineerType::AvionicsEngineer);
    let plane_p = Uuid::new_v4();
    let point = Point::new(10, 10);
    // The waiting engineer stands closer to the aircraft (a routable cell), so
    // being named for the next job is a verdict on the pool, not on distance.
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), waiting.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), free.id),
        "5,38",
    );

    let waiting_task = ctx.seed_task(
        dispatcher.id,
        Some(waiting.id),
        "assigned",
        Some(plane_p),
        point,
    );
    expect_status(
        ctx.post(
            "/api/tasks/current/waiting-part",
            &waiting.token,
            json!(null),
        )
        .await,
        StatusCode::OK,
    );
    let stored = ctx.task(waiting_task);
    assert_eq!(stored.status, "waiting_part");
    assert!(stored.waiting_part);
    // The pool return itself: the aircraft no longer holds the engineer.
    assert_eq!(
        stored.assigned_engineer, None,
        "waiting_part must release the engineer back to the pool"
    );

    // A new request on a different aircraft takes the freed engineer even
    // though the other one is closer.
    let plane_q = Uuid::new_v4();
    let request_q = create_queued_request(&ctx, &dispatcher.token, plane_q, point).await;
    let assigned_q = expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane_q,
                "request_id": request_q
            }),
        )
        .await,
        StatusCode::OK,
    );
    let pooled = candidate_for(&assigned_q, waiting.id);
    assert_eq!(pooled["eligible"], true, "reason: {}", pooled["reason"]);
    assert_eq!(assigned_q["engineer_uuid"], waiting.id.to_string());
}

/// ТЗ п.21: a risk warning must come with an alternative. The alert names an
/// engineer who can still make the deadline, and that engineer is not the one
/// already holding the request.
#[tokio::test]
async fn sla_alert_names_an_engineer_who_can_still_make_it() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let holder = ctx.engineer(EngineerType::AviationTechnician);
    let spare = ctx.engineer(EngineerType::AviationTechnician);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), spare.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), holder.id),
        "5,12",
    );

    // A real request, so the classifier matches the roster and a pool exists to
    // name an alternative from.
    let plane = Uuid::new_v4();
    let task = create_queued_request(&ctx, &dispatcher.token, plane, Point::new(10, 10)).await;
    expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({ "classifier_text": "уход гироплатформы ИНС",
                    "plane_uuid": plane, "request_id": task }),
        )
        .await,
        StatusCode::OK,
    );

    // The holder cannot make it any more: the deadline is seconds away and the
    // forecast already exceeds it, while a nearby engineer still can.
    let now = chrono::Utc::now();
    diesel::update(tasks::table.find(task))
        .set((
            tasks::assigned_engineer.eq(holder.id),
            // Inside the 300 s pre-deadline window, so the risk rule fires.
            tasks::ends_at.eq(now + chrono::Duration::seconds(280)),
            tasks::eta_seconds.eq(Some(900)),
            tasks::sla_alerted.eq(false),
            tasks::sla_breached.eq(false),
        ))
        .execute(&mut ctx.db())
        .unwrap();

    let body = expect_status(
        ctx.get("/api/alerts", &dispatcher.token).await,
        StatusCode::OK,
    );
    let stored = ctx.task(task);
    let alert = body["alerts"]
        .as_array()
        .expect("alerts array")
        .iter()
        .find(|alert| alert["request_id"] == task.to_string())
        .unwrap_or_else(|| {
            panic!(
                "the risky request is alerted: body {body}, status {}, eta {:?}, ends_at {}",
                stored.status, stored.eta_seconds, stored.ends_at
            )
        })
        .clone();
    let alternative = alert["alternative_engineer_id"]
        .as_str()
        .unwrap_or_else(|| panic!("the alert names an alternative engineer: {alert}"))
        .to_string();
    assert_eq!(alternative, spare.id.to_string());
    assert_ne!(alternative, holder.id.to_string());
    let margin = alert["alternative_margin_seconds"]
        .as_i64()
        .expect("the alert publishes the margin");
    assert!(margin > 0, "the alternative must still make it: {margin}");
    let message = alert["message"].as_str().expect("message");
    assert!(
        message.contains(&alternative),
        "the warning text names the alternative: {message}"
    );
}

/// A task assigned before `selection_basis` existed still accepts: the engineer
/// gets `selection: null` instead of a 500, and the status still moves.
#[tokio::test]
async fn accept_without_a_selection_basis_returns_null_not_500() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let request_id = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "assigned",
        None,
        Point::new(10, 10),
    );
    diesel::update(tasks::table.find(request_id))
        .set(tasks::selection_basis.eq(None::<String>))
        .execute(&mut ctx.db())
        .unwrap();

    let body = expect_status(
        ctx.post("/api/tasks/current/accept", &engineer.token, json!(null))
            .await,
        StatusCode::OK,
    );
    assert_eq!(body["status"], "in_progress");
    assert!(body["selection"].is_null(), "unreadable basis is null");
    assert_eq!(ctx.task(request_id).status, "in_progress");

    // A stored value that is not valid JSON is treated the same way.
    let second = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "assigned",
        None,
        Point::new(10, 10),
    );
    diesel::update(tasks::table.find(second))
        .set(tasks::selection_basis.eq(Some("{not json")))
        .execute(&mut ctx.db())
        .unwrap();
    let body = expect_status(
        ctx.post("/api/tasks/current/accept", &engineer.token, json!(null))
            .await,
        StatusCode::OK,
    );
    assert!(body["selection"].is_null());
    assert_eq!(body["status"], "in_progress");
}

/// ТЗ п.17: moving a request to another engineer replaces the stored
/// justification, so the receiving engineer is never shown the numbers that
/// picked their predecessor.
#[tokio::test]
async fn reassign_rewrites_the_selection_basis_for_the_new_engineer() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let first = ctx.engineer(EngineerType::AvionicsEngineer);
    let second = ctx.engineer(EngineerType::EngineTechnician);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), first.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), second.id),
        "5,30",
    );
    let request_id = ctx.seed_task(
        dispatcher.id,
        Some(first.id),
        "assigned",
        None,
        Point::new(10, 10),
    );
    // The basis of the engineer who holds the request before it is moved.
    diesel::update(tasks::table.find(request_id))
        .set(tasks::selection_basis.eq(Some(serde_json::json!({
            "engineer_id": first.id.to_string(),
            "score": 1.0,
            "eta_seconds": 1,
            "distance": 1,
            "uses_vehicle": false,
            "factors": [],
            "reason": "stale pick",
            "manual": false,
            "chosen_at": "2026-01-01T00:00:00+00:00"
        })
        .to_string())))
        .execute(&mut ctx.db())
        .unwrap();

    expect_status(
        ctx.post(
            &format!("/api/requests/{request_id}/reassign"),
            &dispatcher.token,
            json!({ "engineer_id": second.id, "reason": "shift change" }),
        )
        .await,
        StatusCode::OK,
    );

    let task = ctx.task(request_id);
    assert_eq!(task.assigned_engineer, Some(second.id));
    let stored: SelectionBasis =
        serde_json::from_str(task.selection_basis.as_deref().expect("basis is rewritten")).unwrap();
    assert_eq!(
        stored.engineer_id,
        second.id.to_string(),
        "the basis must name the engineer who now holds the request"
    );
    assert_ne!(stored.reason, "stale pick", "the old pick is gone");
    assert!(stored.manual, "a dispatcher named this engineer");
    assert_eq!(
        stored.eta_seconds,
        task.eta_seconds.expect("eta is rebuilt") as u64
    );
    let sum: f64 = stored.factors.iter().map(|factor| factor.seconds).sum();
    assert!(
        (sum - stored.score).abs() < 1e-6,
        "factors {sum} should sum to score {}",
        stored.score
    );

    // The engineer who receives the job sees their own numbers, not the old ones.
    let body = expect_status(
        ctx.post("/api/tasks/current/accept", &second.token, json!(null))
            .await,
        StatusCode::OK,
    );
    assert_eq!(body["selection"]["engineer_id"], second.id.to_string());
    assert_eq!(body["status"], "in_progress");
}

/// ТЗ п.17: unassigning clears the justification with the engineer, so whoever
/// takes the request next cannot inherit a stale one.
#[tokio::test]
async fn unassign_clears_the_selection_basis() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), engineer.id),
        "5,5",
    );
    let plane_id = Uuid::new_v4();
    let request_id = create_queued_request(&ctx, &dispatcher.token, plane_id, Point::new(10, 10)).await;
    expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane_id,
                "request_id": request_id
            }),
        )
        .await,
        StatusCode::OK,
    );
    assert!(
        ctx.task(request_id).selection_basis.is_some(),
        "assignment stores the basis"
    );

    expect_status(
        ctx.post(
            &format!("/api/requests/{request_id}/unassign"),
            &dispatcher.token,
            json!(null),
        )
        .await,
        StatusCode::OK,
    );

    let task = ctx.task(request_id);
    assert_eq!(task.assigned_engineer, None);
    assert_eq!(task.status, "unassigned");
    assert!(
        task.selection_basis.is_none(),
        "no engineer means no justification to hand out"
    );
}

/// ТЗ п.3.2: a request's clock can be shortened or lengthened, so an SLA breach
/// and a comfortable margin can both be staged on purpose.
#[tokio::test]
async fn shifts_the_deadline_of_an_open_request() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let task = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "in_progress",
        None,
        Point::new(10, 10),
    );
    let before = ctx.task(task).ends_at;

    // Shortening the timer moves the deadline back by exactly that much.
    let shortened = expect_status(
        ctx.post(
            &format!("/api/tasks/{task}/timer"),
            &dispatcher.token,
            json!({ "delta_seconds": -600 }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(shortened["request_id"], task.to_string());
    let after = ctx.task(task).ends_at;
    assert_eq!(
        (before - after).num_seconds(),
        600,
        "the deadline moved back by 600 s: before {before}, after {after}"
    );
    assert_eq!(shortened["ends_at"], after.to_rfc3339());
    assert!(
        shortened["seconds_left"].as_i64().is_some_and(|left| left < 400),
        "the answer reports what is left: {shortened}"
    );

    // Lengthening puts it forward, so a breach can be staged and then undone.
    expect_status(
        ctx.post(
            &format!("/api/tasks/{task}/timer"),
            &dispatcher.token,
            json!({ "delta_seconds": 1800 }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!((ctx.task(task).ends_at - after).num_seconds(), 1800);
    let journal = ctx.journal_count();
    assert!(journal > 0, "a timer change is journalled");

    // A zero shift says nothing, a closed request has no clock to move, an
    // unknown id is not found, and only a dispatcher runs this control.
    expect_status(
        ctx.post(
            &format!("/api/tasks/{task}/timer"),
            &dispatcher.token,
            json!({ "delta_seconds": 0 }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.post(
            &format!("/api/tasks/{}/timer", Uuid::new_v4()),
            &dispatcher.token,
            json!({ "delta_seconds": 60 }),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    expect_status(
        ctx.post(
            &format!("/api/tasks/{task}/timer"),
            &engineer.token,
            json!({ "delta_seconds": 60 }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );

    let done = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "done",
        None,
        Point::new(10, 10),
    );
    diesel::update(tasks::table.find(done))
        .set(tasks::is_active.eq(false))
        .execute(&mut ctx.db())
        .unwrap();
    expect_status(
        ctx.post(
            &format!("/api/tasks/{done}/timer"),
            &dispatcher.token,
            json!({ "delta_seconds": 60 }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
}
