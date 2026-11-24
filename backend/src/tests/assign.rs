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

// ---------------------------------------------------------------------------
// Weighted ranking: AOG and post-arrival bonuses
// ---------------------------------------------------------------------------

use chrono::Utc;
use diesel::prelude::*;

use crate::database::schema;
use crate::models::aircraft_movement::AircraftMovement;

/// Creates and queues a request for an aircraft parked at `point`.
async fn create_request_at(
    ctx: &TestContext,
    dispatcher: &str,
    plane_id: Uuid,
    point: (i64, i64),
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

/// Assigns `request_id` and picks the row of `engineer_id` out of the response.
async fn assign_candidate(
    ctx: &TestContext,
    dispatcher: &str,
    plane_id: Uuid,
    request_id: Uuid,
    engineer_id: Uuid,
) -> serde_json::Value {
    let assigned = expect_status(
        ctx.post(
            "/api/assign",
            dispatcher,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane_id,
                "request_id": request_id
            }),
        )
        .await,
        StatusCode::OK,
    );
    let candidates = assigned["candidates"]
        .as_array()
        .expect("candidates should be an array")
        .clone();
    candidates
        .into_iter()
        .find(|row| row["engineer_id"] == engineer_id.to_string())
        .expect("the engineer should be listed among the candidates")
}

/// Seeds an `aircraft_movements` row for the active map.
fn seed_movement(
    ctx: &TestContext,
    plane_id: Uuid,
    point: (i64, i64),
    arrived_at: chrono::DateTime<Utc>,
    departed_at: Option<chrono::DateTime<Utc>>,
) {
    let movement = AircraftMovement {
        id: Uuid::new_v4(),
        aircraft_id: plane_id,
        map_id: ctx.active_id(),
        plane_x: point.0,
        plane_y: point.1,
        arrived_at,
        departed_at,
    };
    diesel::insert_into(schema::aircraft_movements::table)
        .values(&movement)
        .execute(&mut ctx.db())
        .expect("movement should be inserted");
}

/// The AOG bonus must outweigh a modestly longer walk: a grounded aircraft is
/// served before a nearer healthy one. Fails when the bonus is zeroed.
#[tokio::test]
async fn aog_bonus_lets_a_farther_aircraft_outrank_a_closer_one() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), engineer.id),
        "5,5",
    );

    let near_plane = Uuid::new_v4();
    let near_request = create_request_at(&ctx, &dispatcher.token, near_plane, (10, 10)).await;
    let near = assign_candidate(
        &ctx,
        &dispatcher.token,
        near_plane,
        near_request,
        engineer.id,
    )
    .await;

    let far_plane = Uuid::new_v4();
    ctx.redis_set(&format!("map:{}:aog:{}", ctx.active_id(), far_plane), "1");
    let far_request = create_request_at(&ctx, &dispatcher.token, far_plane, (13, 10)).await;
    let far = assign_candidate(
        &ctx,
        &dispatcher.token,
        far_plane,
        far_request,
        engineer.id,
    )
    .await;

    assert_eq!(near["aog_bonus"].as_f64().unwrap(), 0.0);
    assert_eq!(
        far["aog_bonus"].as_f64().unwrap(),
        120.0,
        "a grounded aircraft with a warehouse stop carries the full AOG bonus"
    );
    assert!(
        far["eta_seconds"].as_u64().unwrap() > near["eta_seconds"].as_u64().unwrap(),
        "the AOG aircraft must be the farther one for this test to mean anything"
    );
    assert!(
        far["score"].as_f64().unwrap() < near["score"].as_f64().unwrap(),
        "the AOG bonus must outweigh the extra travel time and the load of the first task"
    );
}

/// An aircraft that has just arrived outranks a nearer one that has been on the
/// apron for long. Fails when the arrival bonus is zeroed.
#[tokio::test]
async fn a_fresh_arrival_outranks_a_nearer_older_aircraft() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), engineer.id),
        "5,5",
    );

    let old_plane = Uuid::new_v4();
    seed_movement(
        &ctx,
        old_plane,
        (10, 10),
        Utc::now() - chrono::Duration::hours(3),
        None,
    );
    let old_request = create_request_at(&ctx, &dispatcher.token, old_plane, (10, 10)).await;
    let old = assign_candidate(
        &ctx,
        &dispatcher.token,
        old_plane,
        old_request,
        engineer.id,
    )
    .await;

    let fresh_plane = Uuid::new_v4();
    seed_movement(&ctx, fresh_plane, (13, 10), Utc::now(), None);
    let fresh_request = create_request_at(&ctx, &dispatcher.token, fresh_plane, (13, 10)).await;
    let fresh = assign_candidate(
        &ctx,
        &dispatcher.token,
        fresh_plane,
        fresh_request,
        engineer.id,
    )
    .await;

    assert_eq!(old["arrival_bonus"].as_f64().unwrap(), 0.0);
    assert!(
        fresh["arrival_bonus"].as_f64().unwrap() > 80.0,
        "an aircraft that landed seconds ago keeps almost the whole bonus, got {}",
        fresh["arrival_bonus"]
    );
    assert!(
        fresh["eta_seconds"].as_u64().unwrap() > old["eta_seconds"].as_u64().unwrap(),
        "the freshly arrived aircraft must be the farther one"
    );
    assert!(
        fresh["score"].as_f64().unwrap() < old["score"].as_f64().unwrap(),
        "the arrival bonus must outweigh the extra travel time and the load of the first task"
    );
}

/// A departed aircraft is not a fresh arrival: `departed_at` drops the bonus.
#[tokio::test]
async fn a_departed_aircraft_gets_no_arrival_bonus() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), engineer.id),
        "5,5",
    );

    let plane = Uuid::new_v4();
    seed_movement(
        &ctx,
        plane,
        (10, 10),
        Utc::now() - chrono::Duration::minutes(2),
        Some(Utc::now() - chrono::Duration::minutes(1)),
    );
    let request = create_request_at(&ctx, &dispatcher.token, plane, (10, 10)).await;
    let row = assign_candidate(&ctx, &dispatcher.token, plane, request, engineer.id).await;

    assert_eq!(row["arrival_bonus"].as_f64().unwrap(), 0.0);
    assert_eq!(row["aog_bonus"].as_f64().unwrap(), 0.0);
}

/// Without AOG or an arrival the order is still the travel time: the nearest
/// engineer wins and nothing is subtracted.
#[tokio::test]
async fn without_flags_the_ranking_is_still_the_travel_time() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let near = ctx.engineer(EngineerType::AvionicsEngineer);
    let far = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), near.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), far.id),
        "5,12",
    );

    let plane = Uuid::new_v4();
    let request = create_request_at(&ctx, &dispatcher.token, plane, (10, 10)).await;
    let assigned = expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane,
                "request_id": request
            }),
        )
        .await,
        StatusCode::OK,
    );

    assert_eq!(assigned["engineer_uuid"], near.id.to_string());
    let mut scores = Vec::new();
    for row in assigned["candidates"].as_array().unwrap() {
        assert_eq!(row["aog_bonus"].as_f64().unwrap(), 0.0);
        assert_eq!(row["arrival_bonus"].as_f64().unwrap(), 0.0);
        if row["eligible"].as_bool().unwrap() {
            scores.push((row["eta_seconds"].as_u64().unwrap(), row["score"].as_f64().unwrap()));
        }
    }
    assert_eq!(scores.len(), 2, "both engineers must be eligible");
    assert!(
        scores[0].0 < scores[1].0 && scores[0].1 < scores[1].1,
        "candidates are ordered by score, and score is the travel time when no bonus applies"
    );
}

/// The published decomposition has to add up to the published score, including
/// the load penalty of an engineer who already has a task.
#[tokio::test]
async fn candidate_score_is_the_sum_of_its_published_components() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let busy = ctx.engineer(EngineerType::AvionicsEngineer);
    let idle = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), busy.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), idle.id),
        "5,12",
    );

    let first_plane = Uuid::new_v4();
    let first_request = create_request_at(&ctx, &dispatcher.token, first_plane, (10, 10)).await;
    let first = assign_candidate(
        &ctx,
        &dispatcher.token,
        first_plane,
        first_request,
        busy.id,
    )
    .await;

    let plane = Uuid::new_v4();
    let request = create_request_at(&ctx, &dispatcher.token, plane, (12, 12)).await;
    let assigned = expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane,
                "request_id": request
            }),
        )
        .await,
        StatusCode::OK,
    );

    let rows = assigned["candidates"].as_array().unwrap().clone();
    let mut seen_load = false;
    for row in &rows {
        if !row["eligible"].as_bool().unwrap() {
            continue;
        }
        let total = row["eta_seconds"].as_f64().unwrap()
            + row["late_seconds"].as_f64().unwrap()
            + row["load_penalty"].as_f64().unwrap()
            - row["aog_bonus"].as_f64().unwrap()
            - row["arrival_bonus"].as_f64().unwrap();
        let score = row["score"].as_f64().unwrap();
        assert!(
            (score - total).abs() < 1e-6,
            "score {score} must equal the sum of the components {total}"
        );
        if row["engineer_id"] == busy.id.to_string() {
            assert_eq!(row["load_penalty"].as_f64().unwrap(), 20.0);
            seen_load = true;
        }
    }
    assert!(seen_load, "the busy engineer must be listed and eligible");

    let row_total = first["eta_seconds"].as_f64().unwrap()
        + first["late_seconds"].as_f64().unwrap()
        + first["load_penalty"].as_f64().unwrap()
        - first["aog_bonus"].as_f64().unwrap()
        - first["arrival_bonus"].as_f64().unwrap();
    assert!((first["score"].as_f64().unwrap() - row_total).abs() < 1e-6);
}

/// B1: the preview ranks the engineers and writes nothing. Two calls in a row
/// must leave the `tasks` table exactly as it was.
#[tokio::test]
async fn candidates_preview_is_read_only() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let near = ctx.engineer(EngineerType::AvionicsEngineer);
    let far = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), near.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), far.id),
        "5,40",
    );
    let plane = Uuid::new_v4();
    let request = create_request_at(&ctx, &dispatcher.token, plane, (10, 10)).await;

    let body = json!({
        "classifier_text": "уход гироплатформы ИНС",
        "plane_uuid": plane,
        "request_id": request
    });
    let first = expect_status(
        ctx.post("/api/assign/candidates", &dispatcher.token, body.clone())
            .await,
        StatusCode::OK,
    );
    let snapshot = ctx.task(request);
    let second = expect_status(
        ctx.post("/api/assign/candidates", &dispatcher.token, body).await,
        StatusCode::OK,
    );
    let after = ctx.task(request);

    assert_eq!(snapshot.status, "unassigned");
    assert_eq!(after.status, "unassigned");
    assert_eq!(after.assigned_engineer, None);
    assert_eq!(after.task_route, snapshot.task_route);
    assert_eq!(after.eta_seconds, snapshot.eta_seconds);

    let rows = second["candidates"].as_array().expect("candidates array");
    assert_eq!(rows.len(), 2);
    assert_eq!(first["candidates"].as_array().unwrap().len(), rows.len());
    assert_eq!(rows[0]["engineer_id"], near.id.to_string());
    assert!(rows[0]["score"].as_f64().unwrap() < rows[1]["score"].as_f64().unwrap());
    assert_eq!(second["request_id"], request.to_string());
    assert_eq!(second["chapter"], "30");
}

/// B2: a named engineer gets the job even when a nearer one is eligible, and
/// the refunded engineer receives the route and the ETA built for them.
#[tokio::test]
async fn manual_assignment_takes_the_named_engineer() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let near = ctx.engineer(EngineerType::AvionicsEngineer);
    let named = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), near.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), named.id),
        "5,38",
    );
    let plane = Uuid::new_v4();
    let request = create_request_at(&ctx, &dispatcher.token, plane, (10, 10)).await;

    let assigned = expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane,
                "request_id": request,
                "engineer_id": named.id
            }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(assigned["engineer_uuid"], named.id.to_string());
    let task = ctx.task(request);
    assert_eq!(task.assigned_engineer, Some(named.id));
    assert_eq!(task.status, "assigned");
    assert_ne!(task.task_route, "[]");
    assert!(task.eta_seconds.unwrap() > 0);
}

/// B2: an engineer who is not admitted to the task's ATA chapter is refused,
/// identified by id or not.
#[tokio::test]
async fn manual_assignment_refuses_an_unadmitted_engineer() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engine = ctx.engineer(EngineerType::EngineTechnician);
    let avionics = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), engine.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), avionics.id),
        "5,6",
    );
    let plane = Uuid::new_v4();
    // Chapter 72 admits engine technicians only.
    let created = expect_status(
        ctx.post(
            "/api/requests",
            &dispatcher.token,
            json!({
                "text": "двигатель: падение давления масла",
                "plane_uuid": plane,
                "point": [10, 10],
                "ata_chapter": "72"
            }),
        )
        .await,
        StatusCode::OK,
    );
    let request: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    expect_status(
        ctx.post(
            &format!("/api/requests/{request}/queue"),
            &dispatcher.token,
            json!(null),
        )
        .await,
        StatusCode::OK,
    );

    expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "двигатель: падение давления масла",
                "plane_uuid": plane,
                "request_id": request,
                "engineer_id": avionics.id
            }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_eq!(ctx.task(request).assigned_engineer, None);
    assert_eq!(ctx.task(request).status, "unassigned");
}

/// B3: a job in `assigned` moves to another engineer, and the route and the
/// ETA are rebuilt for the one who receives it.
#[tokio::test]
async fn reassignment_leaves_assigned_and_rebuilds_the_route() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let first = ctx.engineer(EngineerType::AvionicsEngineer);
    let second = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), first.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), second.id),
        "5,38",
    );
    let plane = Uuid::new_v4();
    let request = create_request_at(&ctx, &dispatcher.token, plane, (10, 10)).await;

    let assigned = expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane,
                "request_id": request
            }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(assigned["engineer_uuid"], first.id.to_string());
    let before = ctx.task(request).eta_seconds.expect("eta is set");

    expect_status(
        ctx.post(
            &format!("/api/requests/{request}/reassign"),
            &dispatcher.token,
            json!({ "engineer_id": second.id, "reason": "ближе по смене" }),
        )
        .await,
        StatusCode::OK,
    );

    let task = ctx.task(request);
    assert_eq!(task.assigned_engineer, Some(second.id));
    assert_eq!(task.status, "assigned");
    let after = task.eta_seconds.expect("eta is set");
    assert_ne!(after, before, "the ETA must follow the new engineer");
    assert_ne!(task.task_route, "[]");
}

/// B3: a job that has already started may still be moved — `in_progress` is
/// not terminal for the dispatcher.
#[tokio::test]
async fn reassignment_leaves_in_progress() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let first = ctx.engineer(EngineerType::AvionicsEngineer);
    let second = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), first.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), second.id),
        "5,38",
    );
    let plane = Uuid::new_v4();
    let request = create_request_at(&ctx, &dispatcher.token, plane, (10, 10)).await;
    expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane,
                "request_id": request
            }),
        )
        .await,
        StatusCode::OK,
    );
    expect_status(
        ctx.post("/api/tasks/current/accept", &first.token, json!(null))
            .await,
        StatusCode::OK,
    );
    assert_eq!(ctx.task(request).status, "in_progress");

    expect_status(
        ctx.post(
            &format!("/api/requests/{request}/reassign"),
            &dispatcher.token,
            json!({ "engineer_id": second.id, "reason": "передаю смене" }),
        )
        .await,
        StatusCode::OK,
    );
    let task = ctx.task(request);
    assert_eq!(task.assigned_engineer, Some(second.id));
    assert_eq!(task.status, "assigned");
}

/// B4: unassign clears the engineer and the route, the job returns to the
/// pool and appears in the preview; a second unassign is a 400.
#[tokio::test]
async fn unassign_returns_the_request_to_the_pool_once() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AvionicsEngineer);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), engineer.id),
        "5,5",
    );
    let plane = Uuid::new_v4();
    let request = create_request_at(&ctx, &dispatcher.token, plane, (10, 10)).await;
    expect_status(
        ctx.post(
            "/api/assign",
            &dispatcher.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane,
                "request_id": request
            }),
        )
        .await,
        StatusCode::OK,
    );

    expect_status(
        ctx.post(
            &format!("/api/requests/{request}/unassign"),
            &dispatcher.token,
            json!(null),
        )
        .await,
        StatusCode::OK,
    );
    let task = ctx.task(request);
    assert_eq!(task.status, "unassigned");
    assert_eq!(task.assigned_engineer, None);
    assert_eq!(task.task_route, "[]");
    assert_eq!(task.eta_seconds, None);

    let preview = expect_status(
        ctx.post(
            "/api/assign/candidates",
            &dispatcher.token,
            json!({
                "classifier_text": "уход гироплатформы ИНС",
                "plane_uuid": plane,
                "request_id": request
            }),
        )
        .await,
        StatusCode::OK,
    );
    let listed = preview["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["engineer_id"] == engineer.id.to_string());
    assert!(listed, "the freed engineer must be ranked again");
    assert_eq!(
        preview["candidates"][0]["eligible"].as_bool(),
        Some(true),
        "the engineer is free after the unassign"
    );

    expect_status(
        ctx.post(
            &format!("/api/requests/{request}/unassign"),
            &dispatcher.token,
            json!(null),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
}

/// B3: `waiting_part` is not terminal either — the dispatcher may hand the job
/// to somebody else while the first engineer waits for a part.
#[tokio::test]
async fn reassignment_leaves_waiting_part() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let waiting = ctx.engineer(EngineerType::EngineTechnician);
    let replacement = ctx.engineer(EngineerType::EngineTechnician);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), waiting.id),
        "5,5",
    );
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), replacement.id),
        "5,30",
    );
    let request = ctx.seed_task(
        dispatcher.id,
        Some(waiting.id),
        "waiting_part",
        None,
        crate::types::map::Point::new(10, 10),
    );

    expect_status(
        ctx.post(
            &format!("/api/requests/{request}/reassign"),
            &dispatcher.token,
            json!({ "engineer_id": replacement.id, "reason": "запчасть привезут позже" }),
        )
        .await,
        StatusCode::OK,
    );
    let task = ctx.task(request);
    assert_eq!(task.assigned_engineer, Some(replacement.id));
    assert_eq!(task.status, "assigned");
    assert!(!task.waiting_part);
    assert!(task.eta_seconds.unwrap() > 0);
    assert_ne!(task.task_route, "[]");
}

/// B4: unassign also works on a started job, and the first engineer stops
/// holding it.
#[tokio::test]
async fn unassign_leaves_in_progress_and_clears_the_engineer() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::EngineTechnician);
    ctx.redis_set(
        &format!("map:{}:position:{}", ctx.active_id(), engineer.id),
        "5,5",
    );
    let request = ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "in_progress",
        None,
        crate::types::map::Point::new(10, 10),
    );

    expect_status(
        ctx.post(
            &format!("/api/requests/{request}/unassign"),
            &dispatcher.token,
            json!(null),
        )
        .await,
        StatusCode::OK,
    );
    let task = ctx.task(request);
    assert_eq!(task.status, "unassigned");
    assert_eq!(task.assigned_engineer, None);
    assert_eq!(task.task_route, "[]");
    assert_eq!(task.eta_seconds, None);
    assert_eq!(task.candidate_ms, None);
}
