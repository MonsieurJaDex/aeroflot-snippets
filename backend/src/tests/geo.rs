//! Geo endpoints: engineer positions, special vehicles and track ingest.

use axum::http::{Method, StatusCode};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::database::schema::tasks;
use crate::tests::support::{expect_status, TestContext};
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
async fn rejects_a_position_batch_addressed_to_another_engineer() {
    let ctx = TestContext::new().await;
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let other = ctx.engineer(EngineerType::AviationTechnician);
    let other_key = format!("map:{}:position:{}", ctx.active_id(), other.id);

    expect_status(
        ctx.post(
            "/api/geo/updateEngineerPosition",
            &engineer.token,
            json!({ "id": other.id, "batch": [[[9, 9], 500]] }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
    assert_eq!(ctx.redis_get(&other_key), None);

    let own_key = format!("map:{}:position:{}", ctx.active_id(), engineer.id);
    expect_status(
        ctx.post(
            "/api/geo/updateEngineerPosition",
            &engineer.token,
            json!({ "id": engineer.id, "batch": [[[4, 4], 600]] }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(ctx.redis_get(&own_key), Some("4,4".to_owned()));
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
        stamp
            .split('.')
            .nth(1)
            .is_some_and(|fraction| fraction.len() >= 4),
        "t should carry milliseconds, got {stamp}"
    );

    let track_key = format!("map:{}:track:e-01", ctx.active_id());
    let stored = ctx.redis_lindex(&track_key, 0).expect("track should be stored");
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

/// Every point of an object is kept, not just the last one: two objects and one
/// object reporting twice must stay independently readable, and the key must
/// expire so a silent tracker stops counting as current.
#[tokio::test]
async fn keeps_every_track_point_of_each_object_under_its_own_key() {
    let ctx = TestContext::new().await;
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let dispatcher = ctx.dispatcher();

    let first = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": engineer.id, "lat": 55.8310, "lon": 37.6360,
                         "t": "2026-12-03T09:14:20.000Z" })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(first["applied"], true);
    let second = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": engineer.id, "lat": 55.8320, "lon": 37.6370,
                         "t": "2026-12-03T09:15:20.000Z" })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(second["applied"], true);
    let other = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "v-01", "lat": 55.8330, "lon": 37.6380,
                         "t": "2026-12-03T09:15:20.000Z" })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(other["applied"], true);

    let key = format!("map:{}:track:{}", ctx.active_id(), engineer.id);
    let other_key = format!("map:{}:track:v-01", ctx.active_id());
    // Newest first, and the older point is still there instead of being overwritten.
    let newest = ctx.redis_lindex(&key, 0).expect("newest point");
    let older = ctx.redis_lindex(&key, 1).expect("the first point is kept");
    assert!(newest.contains("\"lat\":55.832"), "got {newest}");
    assert!(older.contains("\"lat\":55.831"), "got {older}");
    assert_eq!(ctx.redis_lindex(&key, 2), None, "only two points were sent");

    // A different object writes its own key, so simultaneous movement does not mix.
    let independent = ctx.redis_lindex(&other_key, 0).expect("other object track");
    assert!(independent.contains("\"lat\":55.833"), "got {independent}");

    // A tiny lifetime keeps the coordinate current instead of keeping it forever.
    let ttl = ctx.redis_ttl(&key);
    assert!(ttl > 0, "track key must expire, ttl {ttl}");
    assert!(ttl <= 120, "ttl must be short, got {ttl}");
    assert!(ctx.redis_ttl(&other_key) > 0, "track key must expire");

    // Nobody's coordinate disappears from the answer while the key is alive.
    let fresh = expect_status(
        ctx.get(
            &format!("/api/geo/trackFreshness?obj={}", engineer.id),
            &dispatcher.token,
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(fresh["fresh"], true);
    assert!(
        fresh["expires_in_secs"].as_i64().unwrap_or(0) > 0,
        "got {fresh}"
    );
}

/// An object nobody reports is stale rather than an error, and a blank `obj` is
/// rejected instead of silently answered.
#[tokio::test]
async fn track_freshness_reports_a_missing_coordinate_as_stale() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();

    let unknown = expect_status(
        ctx.get("/api/geo/trackFreshness?obj=nobody", &dispatcher.token)
            .await,
        StatusCode::OK,
    );
    assert_eq!(unknown["fresh"], false);
    assert_eq!(unknown["expires_in_secs"], 0);

    let blank = ctx
        .get("/api/geo/trackFreshness?obj=%20", &dispatcher.token)
        .await;
    assert_eq!(blank.0, StatusCode::BAD_REQUEST);

    let no_param = ctx.get("/api/geo/trackFreshness", &dispatcher.token).await;
    assert_eq!(no_param.0, StatusCode::UNPROCESSABLE_ENTITY);

    // Auth is required like every other protected route.
    let anonymous = ctx.get("/api/geo/trackFreshness?obj=nobody", "").await;
    assert_eq!(anonymous.0, StatusCode::UNAUTHORIZED);
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
        .raw_bytes(
            Method::POST,
            "/api/v1/track",
            None,
            point.clone(),
            Some("text/plain"),
        )
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
    let kept = ctx.redis_lindex(&track_key, 0).expect("track should be stored");

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
    assert_eq!(ctx.redis_lindex(&track_key, 0).as_deref(), Some(kept.as_str()));

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
    assert_eq!(ctx.redis_lindex(&track_key, 0).as_deref(), Some(kept.as_str()));

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
    let point = expect_status(
        ctx.request(Method::GET, callback, None, None).await,
        StatusCode::OK,
    );
    assert_eq!(point["obj"], "e-01");
    assert_eq!(point["applied"], true);
    assert_eq!(point["t"], "2025-11-24T16:00:00.000Z");
    let stored = ctx.redis_lindex(&track_key, 0).expect("track should be stored");
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
    assert!(ctx
        .redis_lindex(&track_key, 0)
        .expect("track should be stored")
        .contains("\"lat\":55.8312"));

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

/// A cell of the active map that carries a graph node, together with its
/// WGS-84 form. Picking it from the graph keeps the test independent of the
/// bundled map, whose nodes differ between `assets/map2.json` and the v2 maps.
fn graph_cell_and_geo(ctx: &TestContext) -> (Point, f64, f64) {
    let active = ctx.state.active.read().expect("map lock");
    let graph = &active.location.graph;
    let cell = &active.location.objects[0];
    let point = Point(cell.x, cell.y);
    assert!(
        graph.is_node(point),
        "the first map object is expected to sit on a graph node, got {point:?}"
    );
    let (lat, lon) = graph.project(point.0, point.1);
    (point, lat, lon)
}

#[tokio::test]
async fn calibrated_object_reports_the_pinned_cell() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let (point, lat, lon) = graph_cell_and_geo(&ctx);
    let position_key = format!("map:{}:position:{}", ctx.active_id(), engineer.id);

    let entry = expect_status(
        ctx.post(
            "/api/geo/calibrateObject",
            &dispatcher.token,
            json!({ "obj": engineer.id, "kind": "engineer", "point": [point.0, point.1] }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(entry["obj"], engineer.id.to_string());
    assert_eq!(entry["kind"], "engineer");
    assert_eq!(entry["point"], json!([point.0, point.1]));
    assert_eq!(entry["on_graph"], true);
    // `project` answers `(lat, lon)`, in that order.
    assert!((entry["geo"]["lat"].as_f64().unwrap() - lat).abs() < 1e-9);
    assert!((entry["geo"]["lon"].as_f64().unwrap() - lon).abs() < 1e-9);

    // A tracker point far from the pin is still ingested, and the stored
    // position is the pin rather than the snapped cell. `t` is explicit so the
    // jump passes the ingest's own speed check.
    let far = json!({
        "obj": engineer.id,
        "lat": lat + 0.01,
        "lon": lon + 0.01,
        "t": "2026-11-24T08:00:00Z"
    });
    let ack = expect_status(
        ctx.request(Method::POST, "/api/v1/track", None, Some(far))
            .await,
        StatusCode::OK,
    );
    assert_eq!(ack["applied"], true);
    assert_eq!(
        ctx.redis_get(&position_key),
        Some(point.as_value()),
        "the pin should override the snapped cell"
    );
    // The raw report stays raw: the pin fixes the map cell, not the tracker.
    let stored = ctx
        .redis_lindex(&format!("map:{}:track:{}", ctx.active_id(), engineer.id), 0)
        .expect("track should be stored");
    let stored: serde_json::Value = serde_json::from_str(&stored).expect("track json");
    assert_eq!(
        stored["x"],
        json!(point.0),
        "the pin should be the stored cell"
    );
    // The raw report is what the tracker sent, not the pin.
    assert!((stored["lat"].as_f64().unwrap() - (lat + 0.01)).abs() < 1e-9);
    assert!((stored["lon"].as_f64().unwrap() - (lon + 0.01)).abs() < 1e-9);

    // The pin is listed with the map's own total.
    let listed = expect_status(
        ctx.get("/api/geo/getObjectCalibrations", &engineer.token)
            .await,
        StatusCode::OK,
    );
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["calibrations"][0]["obj"], engineer.id.to_string());

    // Clearing puts the object back on the snapped cell.
    expect_status(
        ctx.post(
            "/api/geo/clearObjectCalibration",
            &dispatcher.token,
            json!({ "obj": engineer.id }),
        )
        .await,
        StatusCode::NO_CONTENT,
    );
    assert_eq!(ctx.redis_get(&position_key), Some(point.as_value()));

    let later = json!({
        "obj": engineer.id,
        "lat": lat + 0.02,
        "lon": lon + 0.02,
        "t": "2026-11-24T09:00:00Z"
    });
    expect_status(
        ctx.request(Method::POST, "/api/v1/track", None, Some(later))
            .await,
        StatusCode::OK,
    );
    let snapped = ctx
        .redis_get(&position_key)
        .expect("position should be stored");
    assert_ne!(
        snapped,
        point.as_value(),
        "without a pin the snapped cell should win again"
    );

    // A second clear finds nothing to remove.
    expect_status(
        ctx.post(
            "/api/geo/clearObjectCalibration",
            &dispatcher.token,
            json!({ "obj": engineer.id }),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn calibration_requires_a_dispatcher_and_a_usable_target() {
    let ctx = TestContext::new().await;
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let (point, _lat, _lon) = graph_cell_and_geo(&ctx);

    // An engineer may not calibrate.
    let denied = expect_status(
        ctx.post(
            "/api/geo/calibrateObject",
            &engineer.token,
            json!({ "obj": engineer.id, "kind": "engineer", "point": [point.0, point.1] }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
    assert_eq!(denied["code"], "NOT_A_DISPATCHER");
    expect_status(
        ctx.post(
            "/api/geo/clearObjectCalibration",
            &engineer.token,
            json!({ "obj": engineer.id }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );

    let dispatcher = ctx.dispatcher();

    // Outside the map bounds.
    let (width, height) = {
        let active = ctx.state.active.read().expect("map lock");
        (active.map.width() as i64, active.map.height() as i64)
    };
    expect_status(
        ctx.post(
            "/api/geo/calibrateObject",
            &dispatcher.token,
            json!({ "obj": "e-01", "kind": "engineer", "point": [width, height] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    // Neither a cell nor a coordinate pair.
    expect_status(
        ctx.post(
            "/api/geo/calibrateObject",
            &dispatcher.token,
            json!({ "obj": "e-01", "kind": "engineer" }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    // Both at once.
    let (_, lat, lon) = graph_cell_and_geo(&ctx);
    expect_status(
        ctx.post(
            "/api/geo/calibrateObject",
            &dispatcher.token,
            json!({
                "obj": "e-01",
                "kind": "engineer",
                "point": [point.0, point.1],
                "lat": lat,
                "lon": lon
            }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    // Only half a coordinate pair.
    expect_status(
        ctx.post(
            "/api/geo/calibrateObject",
            &dispatcher.token,
            json!({ "obj": "e-01", "kind": "engineer", "lat": lat }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
}

#[tokio::test]
async fn calibration_accepts_wgs84_and_rounds_it_to_the_nearest_cell() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let (point, lat, lon) = graph_cell_and_geo(&ctx);

    let entry = expect_status(
        ctx.post(
            "/api/geo/calibrateObject",
            &dispatcher.token,
            json!({ "obj": "e-01", "kind": "engineer", "lat": lat, "lon": lon }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(entry["point"], json!([point.0, point.1]));

    expect_status(
        ctx.post(
            "/api/geo/clearObjectCalibration",
            &dispatcher.token,
            json!({ "obj": "e-01" }),
        )
        .await,
        StatusCode::NO_CONTENT,
    );
    assert!(!ctx.redis_exists(&format!("map:{}:calibration:e-01", ctx.active_id())));
}

#[test]
fn unproject_inverts_the_projection() {
    let graph = crate::graph::GraphBuilder::new(12.0, 55.83, 37.63).finish();
    for (x, y) in [(0_i64, 0_i64), (1, 1), (17, 43), (120, 7)] {
        let (lat, lon) = graph.project(x, y);
        let (back_x, back_y) = graph.unproject(lat, lon);
        assert!(
            (back_x - x as f64).abs() < 1e-6 && (back_y - y as f64).abs() < 1e-6,
            "cell ({x},{y}) should survive project/unproject, got ({back_x},{back_y})"
        );
    }
}

const REAL_MAP: &str = "assets/it2026_aero.json";

/// A cell inside the map bounds that carries no graph node. The palette map
/// leaves obstacles and unknown cells out of the routing graph, so a pin there
/// would be silently moved by `nearest_node` — which is what the handler refuses.
fn cell_without_node(ctx: &TestContext) -> Point {
    let active = ctx.state.active.read().expect("map lock");
    let graph = &active.location.graph;
    for y in 0..active.map.height() as i64 {
        for x in 0..active.map.width() as i64 {
            let candidate = Point(x, y);
            if active.map.in_bounds(candidate) && !graph.is_node(candidate) {
                return candidate;
            }
        }
    }
    panic!("the palette map is expected to leave cells out of the routing graph");
}

#[tokio::test]
async fn calibration_refuses_a_cell_that_carries_no_graph_node() {
    let ctx = TestContext::with_map(Some(REAL_MAP)).await;
    let dispatcher = ctx.dispatcher();
    let bare = cell_without_node(&ctx);

    expect_status(
        ctx.post(
            "/api/geo/calibrateObject",
            &dispatcher.token,
            json!({ "obj": "e-01", "kind": "engineer", "point": [bare.0, bare.1] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert!(ctx
        .redis_get(&format!("map:{}:calibration:e-01", ctx.active_id()))
        .is_none());

    // A node of the same map is accepted, so the refusal is about the cell and
    // not about the request.
    let node = {
        let active = ctx.state.active.read().expect("map lock");
        let graph = &active.location.graph;
        (0..active.map.height() as i64)
            .flat_map(|y| (0..active.map.width() as i64).map(move |x| Point(x, y)))
            .find(|candidate| graph.is_node(*candidate))
            .expect("the palette map is expected to have graph nodes")
    };
    expect_status(
        ctx.post(
            "/api/geo/calibrateObject",
            &dispatcher.token,
            json!({ "obj": "e-01", "kind": "engineer", "point": [node.0, node.1] }),
        )
        .await,
        StatusCode::OK,
    );
}

/// ТЗ п.20: the forecast is live, not written once at assignment. When the
/// engineer's position moves into a new cell, the tracker refreshes the task's
/// `eta_seconds`; a second point inside the same cell costs no new route.
#[tokio::test]
async fn track_refresh_updates_the_task_forecast_only_when_the_cell_changes() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    // A routable cell, far from the aircraft on the same apron as the other
    // assignment tests use.
    let plane = Point::new(5, 38);
    let task = ctx.seed_task(dispatcher.id, Some(engineer.id), "assigned", None, plane);
    // A forecast that is deliberately wrong before the first point: anything
    // the tracker writes is a refresh, not a value that was already there.
    diesel::update(tasks::table.find(task))
        .set(tasks::eta_seconds.eq(Some(0)))
        .execute(&mut ctx.db())
        .unwrap();

    let graph = &ctx.state.active.read().unwrap().location.graph;
    let (far_lat, far_lon) = graph.project(5, 30);
    let (near_lat, near_lon) = graph.project(5, 5);
    // Each point is minutes after the previous one, so a move between two
    // distant cells is not judged a teleport by the speed check.
    let stamp_after = |minutes: i64| {
        (chrono::Utc::now() + chrono::Duration::minutes(minutes))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };

    let moved = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(
                json!({ "obj": engineer.id.to_string(), "lat": far_lat, "lon": far_lon,
                         "t": stamp_after(0) }),
            ),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(moved["applied"], true, "ack: {moved}");
    // The ack contract is unchanged: exactly these four fields.
    let ack = moved.as_object().expect("ack is an object");
    let mut keys: Vec<&str> = ack.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["accepted", "applied", "obj", "t"]);

    let after_first = ctx.task(task).eta_seconds.expect("forecast was written");
    assert!(
        after_first > 0,
        "the tracker must recompute the forecast, not keep the seeded zero"
    );

    // A second point inside the same cell must not touch the forecast: the
    // database value is poisoned to prove no route ran.
    diesel::update(tasks::table.find(task))
        .set(tasks::eta_seconds.eq(Some(-5)))
        .execute(&mut ctx.db())
        .unwrap();
    let same_cell = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(
                json!({ "obj": engineer.id.to_string(), "lat": far_lat, "lon": far_lon,
                         "t": stamp_after(1) }),
            ),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(same_cell["applied"], true, "ack: {same_cell}");
    assert_eq!(
        ctx.task(task).eta_seconds,
        Some(-5),
        "a point inside the same cell must not recompute the route"
    );

    // The next cell refreshes it again.
    let next_cell = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(
                json!({ "obj": engineer.id.to_string(), "lat": near_lat, "lon": near_lon,
                         "t": stamp_after(2) }),
            ),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(next_cell["applied"], true, "ack: {next_cell}");
    assert!(
        ctx.task(task).eta_seconds.is_some_and(|eta| eta > 0),
        "a new cell must refresh the forecast again, got {:?}",
        ctx.task(task).eta_seconds
    );
}

/// ТЗ п.3.1: an agent can be stopped without ending its route, so a halted
/// engineer can be reproduced. While paused its points are accepted but not
/// applied; resuming lets them through again.
#[tokio::test]
async fn pauses_and_resumes_an_agent_without_losing_its_route() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let paused_key = format!("map:{}:paused:e-01", ctx.active_id());
    let track_key = format!("map:{}:track:e-01", ctx.active_id());

    let paused = expect_status(
        ctx.post(
            "/api/geo/pauseAgent",
            &dispatcher.token,
            json!({ "obj": "e-01", "paused": true }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(paused["obj"], "e-01");
    assert_eq!(paused["paused"], true);
    assert_eq!(ctx.redis_get(&paused_key), Some("1".to_owned()));

    let while_paused = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "e-01", "lat": 55.831, "lon": 37.636 })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(while_paused["accepted"], true, "ack: {while_paused}");
    assert_eq!(while_paused["applied"], false, "ack: {while_paused}");
    assert_eq!(
        ctx.redis_lindex(&track_key, 0),
        None,
        "a paused agent must not move"
    );

    expect_status(
        ctx.post(
            "/api/geo/pauseAgent",
            &dispatcher.token,
            json!({ "obj": "e-01", "paused": false }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(
        ctx.redis_get(&paused_key),
        None,
        "resuming removes the flag instead of leaving a stale value"
    );

    let after_resume = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "e-01", "lat": 55.832, "lon": 37.637 })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(after_resume["applied"], true, "ack: {after_resume}");
    assert!(ctx.redis_lindex(&track_key, 0).is_some());

    // Only a dispatcher drives the test controls, and a blank object is a
    // malformed request rather than a silent pause of nothing.
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    expect_status(
        ctx.post(
            "/api/geo/pauseAgent",
            &engineer.token,
            json!({ "obj": "e-01", "paused": true }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
    expect_status(
        ctx.post(
            "/api/geo/pauseAgent",
            &dispatcher.token,
            json!({ "obj": "  ", "paused": true }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
}

/// ТЗ п.3.3: an engineer can be forced onto a vehicle so the vehicle branch of a
/// candidate can be shown even when walking would be faster.
#[tokio::test]
async fn forces_and_releases_the_vehicle_choice_of_an_engineer() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    let key = format!("rank:force_vehicle:{}", engineer.id);

    let forced = expect_status(
        ctx.post(
            "/api/geo/forceVehicle",
            &dispatcher.token,
            json!({ "engineer_id": engineer.id, "forced": true }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(forced["engineer_id"], engineer.id.to_string());
    assert_eq!(forced["forced"], true);
    assert_eq!(ctx.redis_get(&key), Some("1".to_owned()));

    let released = expect_status(
        ctx.post(
            "/api/geo/forceVehicle",
            &dispatcher.token,
            json!({ "engineer_id": engineer.id, "forced": false }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(released["forced"], false);
    assert_eq!(ctx.redis_get(&key), None, "releasing clears the flag");

    expect_status(
        ctx.post(
            "/api/geo/forceVehicle",
            &dispatcher.token,
            json!({ "engineer_id": Uuid::new_v4(), "forced": true }),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    expect_status(
        ctx.post(
            "/api/geo/forceVehicle",
            &engineer.token,
            json!({ "engineer_id": engineer.id, "forced": true }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
}

/// The organiser geodata names its objects `AT-*`, `vehicle-*` and `RA-*`, which
/// the bundled map does not list; every such id must still be accepted, while a
/// genuinely unknown id stays rejected.
#[tokio::test]
async fn accepts_organiser_object_ids_from_the_geodata() {
    let ctx = TestContext::new().await;

    // The kind of an id the map does not list is judged by the id shape, and the
    // shared position key is what it lands under: an engineer gets the plain
    // engineer key, a vehicle and an aircraft the typed vehicle key.
    for (obj, expected_key) in [
        ("AT-0001", format!("map:{}:position:{{id}}", ctx.active_id())),
        ("AT-0007", format!("map:{}:position:{{id}}", ctx.active_id())),
        (
            "vehicle-01",
            format!("map:{}:position:fuel_truck:{{id}}", ctx.active_id()),
        ),
        (
            "vehicle-03",
            format!("map:{}:position:fuel_truck:{{id}}", ctx.active_id()),
        ),
        (
            "RA-73101",
            format!("map:{}:position:aircraft:{{id}}", ctx.active_id()),
        ),
        (
            "RA-73110",
            format!("map:{}:position:aircraft:{{id}}", ctx.active_id()),
        ),
    ] {
        let ack = expect_status(
            ctx.request(
                Method::POST,
                "/api/v1/track",
                None,
                Some(json!({ "obj": obj, "lat": 55.831, "lon": 37.636 })),
            )
            .await,
            StatusCode::OK,
        );
        assert_eq!(ack["applied"], true, "{obj} must be applied: {ack}");
        assert!(
            ctx.redis_lindex(&format!("map:{}:track:{obj}", ctx.active_id()), 0)
                .is_some(),
            "{obj} must be stored"
        );
        let id = Uuid::new_v5(&Uuid::NAMESPACE_OID, obj.as_bytes());
        let position_key = expected_key.replace("{id}", &id.to_string());
        assert!(
            ctx.redis_get(&position_key).is_some(),
            "{obj} must publish its position under {position_key}"
        );
    }

    let unknown = expect_status(
        ctx.request(
            Method::POST,
            "/api/v1/track",
            None,
            Some(json!({ "obj": "X-9999", "lat": 55.831, "lon": 37.636 })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(unknown["accepted"], true);
    assert_eq!(unknown["applied"], false, "ack: {unknown}");
}
