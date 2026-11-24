//! Request lifecycle endpoints: intake, queue, reassignment and engineer actions.

use axum::http::StatusCode;
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::database::schema::tasks;
use crate::tests::support::{TestContext, expect_status};
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
    let second = ctx.engineer(EngineerType::AviationTechnician);
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

    expect_status(
        ctx.post(
            &format!("/api/requests/{request_id}/reassign"),
            &dispatcher.token,
            json!({ "engineer_id": first.id, "reason": "again" }),
        )
        .await,
        StatusCode::BAD_REQUEST,
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
            json!({ "engineer_id": second.id, "reason": "too early" }),
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
    assert!(
        alerts
            .iter()
            .any(|alert| alert["request_id"] == risky.to_string())
    );

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
    let params = csv_fields(defect_row)
        .pop()
        .expect("params column");
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
