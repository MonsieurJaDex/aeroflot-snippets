//! Engineer assignment and current-task AOG endpoints.

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;

use crate::tests::support::{TestContext, expect_status};
use crate::types::enums::EngineerType;
use crate::types::map::Point;

async fn create_queued_request(ctx: &TestContext, dispatcher: &str, plane_id: Uuid) -> Uuid {
    let created = expect_status(
        ctx.post(
            "/api/requests",
            dispatcher,
            json!({
                "text": "уход гироплатформы ИНС, курс смещается",
                "plane_uuid": plane_id,
                "point": [10, 10],
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

#[tokio::test]
async fn assigns_the_nearest_eligible_engineer_and_persists_the_route() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AvionicsEngineer);
    let plane_id = Uuid::new_v4();
    let position_key = format!("map:{}:position:{}", ctx.active_id(), engineer.id);
    ctx.redis_set(&position_key, "5,5");

    let request_id = create_queued_request(&ctx, &dispatcher.token, plane_id).await;

    let assigned = expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "note": "assign this",
                "plane_uuid": plane_id,
                "request_id": request_id
            }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(assigned["engineer_uuid"], engineer.id.to_string());
    assert!(assigned["eta_seconds"].as_u64().unwrap() > 0);
    assert!(assigned["distance"].as_u64().unwrap() > 0);
    assert!(assigned["route"].as_array().unwrap().len() > 1);
    assert_eq!(assigned["time"], assigned["eta_seconds"]);

    let task = ctx.task(request_id);
    assert_eq!(task.status, "assigned");
    assert_eq!(task.assigned_engineer, Some(engineer.id));
    assert_ne!(task.task_route, "[]");
    assert!(task.eta_seconds.is_some());
    assert!(task.candidate_ms.is_some());
    assert!(ctx.journal_count() >= 3);

    // A request can be assigned only once.
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
        StatusCode::BAD_REQUEST,
    );
}

#[tokio::test]
async fn assign_reports_missing_engineers_and_invalid_requests() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AvionicsEngineer);
    let plane_id = Uuid::new_v4();
    let request_id = create_queued_request(&ctx, &dispatcher.token, plane_id).await;

    // No engineer position in Redis means nobody is eligible.
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
        StatusCode::NOT_FOUND,
    );

    expect_status(
        ctx.post(
            "/api/assign",
            &engineer.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane_id,
                "request_id": request_id
            }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );

    expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({ "classifier_text": "no request", "plane_uuid": plane_id }),
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
    expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "bad id",
                "plane_uuid": plane_id,
                "request_id": "not-a-uuid"
            }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "missing",
                "plane_uuid": plane_id,
                "request_id": Uuid::new_v4()
            }),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn toggles_aog_on_the_current_task_and_clears_it_with_the_legacy_post() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::CrewRemarksHandler);
    let plane_id = Uuid::new_v4();
    ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "in_progress",
        Some(plane_id),
        Point::new(10, 10),
    );

    expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &dispatcher.token,
            json!({ "id": plane_id, "point": [10, 10] }),
        )
        .await,
        StatusCode::OK,
    );

    let aog_key = format!("map:{}:aog:{plane_id}", ctx.active_id());
    let body = expect_status(
        ctx.put(
            "/api/tasks/current/aog",
            &engineer.token,
            json!({ "aog": true }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(body["aog"], true);
    assert_eq!(body["plane_uuid"], plane_id.to_string());
    assert!(ctx.redis_exists(&aog_key));

    let list = expect_status(
        ctx.get("/api/tasks/aog", &dispatcher.token).await,
        StatusCode::OK,
    );
    assert_eq!(list["total"], 1);
    assert_eq!(list["aircraft"][0]["id"], plane_id.to_string());
    assert_eq!(list["plane_uuids"], json!([plane_id.to_string()]));

    let cleared = expect_status(
        ctx.post(
            "/api/tasks/current/aog",
            &engineer.token,
            json!({ "toggle": false }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(cleared["aog"], false);
    assert!(!ctx.redis_exists(&aog_key));

    expect_status(
        ctx.put("/api/tasks/current/aog", &engineer.token, json!({}))
            .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
    expect_status(
        ctx.post(
            "/api/tasks/current/aog",
            &dispatcher.token,
            json!({ "aog": true }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
    expect_status(
        ctx.get("/api/tasks/aog", &engineer.token).await,
        StatusCode::FORBIDDEN,
    );

    let idle = ctx.engineer(EngineerType::FuelingCrew);
    expect_status(
        ctx.put(
            "/api/tasks/current/aog",
            &idle.token,
            json!({ "aog": true }),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
}
