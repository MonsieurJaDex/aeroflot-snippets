//! Geo endpoints: engineer positions, special vehicles and track ingest.

use axum::http::{Method, StatusCode};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::tests::support::{TestContext, expect_status};
use crate::types::enums::EngineerType;
use crate::types::map::Point;

#[tokio::test]
async fn applies_buffered_engineer_positions_and_keeps_the_newest() {
    let ctx = TestContext::new().await;
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let position_key = format!("map:{}:position:{}", ctx.active_id(), engineer.id);
    let timestamp_key = format!("map:{}:position_ts:{}", ctx.active_id(), engineer.id);

    let body = expect_status(
        ctx.post(
            "/api/geo/updateEngineerPosition",
            &engineer.token,
            json!({
                "id": engineer.id,
                "batch": [
                    [[5, 5], 100],
                    [[6, 6], 200]
                ]
            }),
        )
        .await,
        StatusCode::OK,
    );
    assert!(body.is_null());
    assert_eq!(ctx.redis_get(&position_key), Some("6,6".to_owned()));
    assert_eq!(ctx.redis_get(&timestamp_key), Some("200".to_owned()));

    // An older buffered batch must not move the engineer backwards.
    expect_status(
        ctx.post(
            "/api/geo/updateEngineerPosition",
            &engineer.token,
            json!({ "id": engineer.id, "batch": [[[7, 7], 150]] }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(ctx.redis_get(&position_key), Some("6,6".to_owned()));

    expect_status(
        ctx.post(
            "/api/geo/updateEngineerPosition",
            &engineer.token,
            json!({ "id": engineer.id, "batch": [[[8, 8], 250]] }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(ctx.redis_get(&position_key), Some("8,8".to_owned()));

    let body = expect_status(
        ctx.get("/api/geo/getEngineersPositions", &engineer.token)
            .await,
        StatusCode::OK,
    );
    assert_eq!(body["total"], 1);
    assert_eq!(body["engineers"][0]["id"], engineer.id.to_string());
    assert_eq!(body["engineers"][0]["position"], json!([8, 8]));
}

#[tokio::test]
async fn validates_engineer_position_batches() {
    let ctx = TestContext::new().await;
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let dispatcher = ctx.dispatcher();

    expect_status(
        ctx.post(
            "/api/geo/updateEngineerPosition",
            &engineer.token,
            json!({ "id": engineer.id, "batch": [] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.post(
            "/api/geo/updateEngineerPosition",
            &engineer.token,
            json!({ "id": engineer.id, "batch": [[[1, 1], -1]] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.post(
            "/api/geo/updateEngineerPosition",
            &engineer.token,
            json!({ "id": engineer.id, "batch": [[[999, 999], 10]] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.post(
            "/api/geo/updateEngineerPosition",
            &engineer.token,
            json!({ "id": "bad-uuid", "batch": [[[1, 1], 10]] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.post(
            "/api/geo/updateEngineerPosition",
            &dispatcher.token,
            json!({ "id": dispatcher.id, "batch": [[[1, 1], 10]] }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
}

#[tokio::test]
async fn stores_and_lists_special_vehicle_positions() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let vehicle_id = Uuid::new_v4();
    let key = format!("map:{}:position:fuel_truck:{vehicle_id}", ctx.active_id());

    expect_status(
        ctx.post(
            "/api/geo/updateTransportPosition",
            &dispatcher.token,
            json!({ "id": vehicle_id, "vehicle_type": "FuelTruck", "new_point": [8, 6] }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(ctx.redis_get(&key), Some("8,6".to_owned()));

    let body = expect_status(
        ctx.get("/api/geo/getTransportPositions", &dispatcher.token)
            .await,
        StatusCode::OK,
    );
    assert_eq!(body["total"], 1);
    assert_eq!(body["transport"][0]["id"], vehicle_id.to_string());
    assert_eq!(body["transport"][0]["vehicle_type"], "FuelTruck");
    assert_eq!(body["transport"][0]["point"], json!([8, 6]));

    expect_status(
        ctx.post(
            "/api/geo/updateTransportPosition",
            &dispatcher.token,
            json!({ "id": vehicle_id, "vehicle_type": "FuelTruck", "new_point": [999, 999] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    let engineer = ctx.engineer(EngineerType::FuelingCrew);
    expect_status(
        ctx.post(
            "/api/geo/updateTransportPosition",
            &engineer.token,
            json!({ "id": vehicle_id, "vehicle_type": "FuelTruck", "new_point": [1, 1] }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
}

#[tokio::test]
async fn lists_engineers_with_active_tasks() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let idle = ctx.engineer(EngineerType::FuelingCrew);

    ctx.seed_task(
        dispatcher.id,
        Some(engineer.id),
        "assigned",
        None,
        Point::new(10, 10),
    );
    let finished = ctx.seed_task(
        dispatcher.id,
        Some(idle.id),
        "done",
        None,
        Point::new(10, 10),
    );
    diesel::update(crate::database::schema::tasks::table.find(finished))
        .set(crate::database::schema::tasks::is_active.eq(false))
        .execute(&mut ctx.db())
        .unwrap();

    let body = expect_status(
        ctx.get("/api/geo/activeEngineers", &dispatcher.token).await,
        StatusCode::OK,
    );
    let ids = body["active_engineers"].as_array().unwrap();
    assert_eq!(ids, &vec![json!(engineer.id.to_string())]);
}

#[tokio::test]
async fn ingests_track_points_for_known_objects_only() {
    let ctx = TestContext::new().await;

    let single = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "e-01", "lat": 55.831, "lon": 37.636 })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(single["accepted"], true);
    assert_eq!(single["applied"], true);
    assert_eq!(single["obj"], "e-01");
    // contract: one point answers with an object, not an array, and `t` is an
    // ISO 8601 UTC string with milliseconds.
    assert!(single.is_object());
    let stamp = single["t"].as_str().expect("t should be a string");
    assert!(stamp.ends_with('Z'), "t should be UTC, got {stamp}");
    assert!(
        stamp.split('.').nth(1).is_some_and(|fraction| fraction.len() >= 4),
        "t should carry milliseconds, got {stamp}"
    );

    let track_key = format!("map:{}:track:e-01", ctx.active_id());
    let stored = ctx.redis_get(&track_key).expect("track should be stored");
    assert!(stored.contains("\"lat\":55.831"));
    assert!(stored.contains("\"x\":"));

    let unknown = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "unknown-object", "lat": 55.831, "lon": 37.636 })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(unknown["accepted"], true);
    assert_eq!(unknown["applied"], false);

    expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "e-01", "lon": 37.636 })),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    let batch = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({
                "points": [
                    { "obj": "unknown-object", "lat": 55.831, "lon": 37.636 },
                    { "obj": "RA-73142", "lat": 55.8311, "lon": 37.6361 }
                ]
            })),
        )
        .await,
        StatusCode::OK,
    );
    let batch = batch.as_array().expect("a batch answers with an array");
    assert_eq!(batch.len(), 2);
    // contract: the batch keeps the request order and echoes each `obj`, mixing a
    // known and an unknown object without failing the whole request.
    assert_eq!(batch[0]["obj"], "unknown-object");
    assert_eq!(batch[0]["applied"], false);
    assert_eq!(batch[1]["obj"], "RA-73142");
    assert_eq!(batch[1]["applied"], true);

    // contract: the shape follows the request, not the number of points, so a
    // one-point batch still answers with an array.
    let single_in_batch = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({
                "points": [{ "obj": "RA-73142", "lat": 55.8312, "lon": 37.6362 }]
            })),
        )
        .await,
        StatusCode::OK,
    );
    let single_in_batch = single_in_batch
        .as_array()
        .expect("a batch answers with an array even when it holds one point");
    assert_eq!(single_in_batch.len(), 1);
    assert_eq!(single_in_batch[0]["obj"], "RA-73142");
}

#[tokio::test]
async fn rejects_track_bodies_that_break_the_post_contract() {
    let ctx = TestContext::new().await;

    // `lat` is a string, not a number.
    expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "e-01", "lat": "abc", "lon": 37.636 })),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    // `lat` is null and then absent: both are a missing required number.
    expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "e-01", "lat": null, "lon": 37.636 })),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "e-01", "lon": 37.636 })),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    // `obj` is required and cannot be blank.
    expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "lat": 55.831, "lon": 37.636 })),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "   ", "lat": 55.831, "lon": 37.636 })),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    // A blank `obj` is rejected before storage, so nothing is written.
    assert!(!ctx.redis_exists(&format!("map:{}:track:e-01", ctx.active_id())));

    // A malformed point inside a batch is dropped; the valid one around it is
    // still applied (contract: одна неверная точка не роняет пакет).
    let mixed = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({
                "points": [
                    { "obj": "e-01", "lat": 55.831, "lon": 37.636 },
                    { "obj": "e-01", "lat": "abc", "lon": 37.636 },
                    { "obj": "e-01", "lon": 37.636 }
                ]
            })),
        )
        .await,
        StatusCode::OK,
    );
    let acks = mixed.as_array().expect("a batch answers with an array");
    assert_eq!(acks.len(), 1);
    assert_eq!(acks[0]["obj"], "e-01");
    assert_eq!(acks[0]["applied"], true);
    assert!(ctx.redis_exists(&format!("map:{}:track:e-01", ctx.active_id())));

    // Malformed JSON is 400, not 500 or axum's 422.
    let point = br#"{"obj":"e-01","lat":55.831,"lon":37.636}"#.to_vec();
    let truncated = br#"{"obj": "e-01", "lat": "#.to_vec();
    let (status, body) = ctx
        .raw_bytes(
            Method::POST,
            "/api/v1/track",
            None,
            truncated,
            Some("application/json"),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");

    // A body that is not declared as JSON is 415.
    let (status, body) = ctx
        .raw_bytes(Method::POST, "/api/v1/track", None, point.clone(), Some("text/plain"))
        .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "body: {body}");
    let (status, body) = ctx
        .raw_bytes(Method::POST, "/api/v1/track", None, point.clone(), None)
        .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "body: {body}");

    // A charset parameter is part of a valid JSON media type.
    let (status, body) = ctx
        .raw_bytes(
            Method::POST,
            "/api/v1/track",
            None,
            point,
            Some("application/json; charset=utf-8"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let ack: serde_json::Value = serde_json::from_str(&body).expect("ack should be JSON");
    assert_eq!(ack["obj"], "e-01");
    assert_eq!(ack["applied"], true);
}

#[tokio::test]
async fn drops_outliers_without_losing_the_previous_point() {
    let ctx = TestContext::new().await;
    let track_key = format!("map:{}:track:e-01", ctx.active_id());
    let seeded = ctx.journal_count();

    let first = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({
                "obj": "e-01",
                "lat": 55.831,
                "lon": 37.636,
                "acc": 5.0,
                "t": "2026-12-03T09:14:20.123Z"
            })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(first["applied"], true);
    let kept = ctx.redis_get(&track_key).expect("track should be stored");

    // contract: an accuracy coarser than `max_accuracy_m` (80 m in map2) is an
    // outlier — accepted syntactically, refused semantically, previous point kept.
    let coarse = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({
                "obj": "e-01",
                "lat": 55.8311,
                "lon": 37.6361,
                "acc": 500.0,
                "t": "2026-12-03T09:14:21.123Z"
            })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(coarse["accepted"], true);
    assert_eq!(coarse["applied"], false);
    assert_eq!(ctx.redis_get(&track_key).as_deref(), Some(kept.as_str()));

    // contract: jumping ~8 km in one second exceeds `max_speed_mps` (12 m/s), so
    // the point is an outlier as well.
    let jump = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({
                "obj": "e-01",
                "lat": 55.9,
                "lon": 37.636,
                "acc": 5.0,
                "t": "2026-12-03T09:14:22.123Z"
            })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(jump["accepted"], true);
    assert_eq!(jump["applied"], false);
    assert_eq!(ctx.redis_get(&track_key).as_deref(), Some(kept.as_str()));

    // Both refusals are journalled, not silently dropped.
    assert_eq!(ctx.journal_count(), seeded + 2);
}

#[tokio::test]
async fn ingests_gpslogger_custom_url_query() {
    let ctx = TestContext::new().await;
    let track_key = format!("map:{}:track:e-01", ctx.active_id());

    // The exact query a GPSLogger Custom URL callback sends for `%ALL`, with the
    // placeholders spelled in upper case the way the app accepts them.
    let callback = "/api/v1/track?obj=e-01&lat=55.8312&lon=37.6361&sat=9&desc=&alt=150.0&acc=5.0&dir=0.0&prov=gps&spd_kph=4.32&spd=1.2&timestamp=1764000000&timeoffset=2025-11-24T16:00:00.000Z&time=2025-11-24T16:00:00.000Z&starttimestamp=1763999000&date=2025-11-24&batt=87.0&ischarging=true&aid=&ser=&act=&filename=20251124&profile=&hdop=0.9&vdop=1.1&pdop=1.4&dist=120";
    let point = expect_status(ctx.request(Method::GET, callback, None, None).await, StatusCode::OK);
    assert_eq!(point["obj"], "e-01");
    assert_eq!(point["applied"], true);
    assert_eq!(point["t"], "2025-11-24T16:00:00.000Z");
    let stored = ctx.redis_get(&track_key).expect("track should be stored");
    assert!(stored.contains("\"lat\":55.8312"));
    assert!(stored.contains("\"acc\":5.0"));
    assert!(stored.contains("\"spd\":1.2"));

    // The app repeats each logged line and queues offline points, so an older
    // sample for the same object is acknowledged without overwriting the newer one.
    let older = expect_status(
        ctx.request(
            Method::GET,
            "/api/v1/track?obj=e-01&lat=55.9&lon=37.7&timestamp=1763999000",
            None,
            None,
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(older["accepted"], true);
    assert_eq!(older["applied"], false);
    assert!(
        ctx.redis_get(&track_key)
            .expect("track should be stored")
            .contains("\"lat\":55.8312")
    );

    // `%time` is the fallback when `%timestamp` is not part of the URL, and a
    // value the device could not measure arrives as `NaN`.
    let fallback = expect_status(
        ctx.request(
            Method::GET,
            "/api/v1/track?obj=RA-73142&lat=55.8311&lon=37.6361&acc=NaN&time=2025-11-24T16:05:30.500Z",
            None,
            None,
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(fallback["applied"], true);
    assert_eq!(fallback["t"], "2025-11-24T16:05:30.500Z");

    // Without coordinates there is nothing to place.
    expect_status(
        ctx.request(
            Method::GET,
            "/api/v1/track?obj=e-01&lat=&lon=37.6361",
            None,
            None,
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
}
