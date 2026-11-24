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
                    { "obj": "e-01", "lat": 55.831, "lon": 37.636 },
                    { "obj": "RA-73142", "lat": 55.8311, "lon": 37.6361 }
                ]
            })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(batch.as_array().unwrap().len(), 2);
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
