//! Map catalog, route calculation and classifier endpoints.

use axum::http::StatusCode;
use serde_json::json;

use crate::tests::support::{TestContext, expect_status};
use crate::types::enums::EngineerType;

#[tokio::test]
async fn serves_map_catalog_and_switches_the_active_map() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();

    let map = expect_status(ctx.get("/api/map", &dispatcher.token).await, StatusCode::OK);
    assert_eq!(map["id"], ctx.active_id());
    assert_eq!(map["matrix"].as_array().unwrap().len(), 64);

    let catalog = expect_status(
        ctx.get("/api/maps", &dispatcher.token).await,
        StatusCode::OK,
    );
    let ids: Vec<&str> = catalog["maps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"map"));
    assert!(ids.contains(&"map2"));
    assert_eq!(catalog["active_id"], "map2");

    let active = expect_status(
        ctx.get("/api/maps/active", &dispatcher.token).await,
        StatusCode::OK,
    );
    assert_eq!(active["id"], "map2");
    assert_eq!(active["width"], 64);

    let switched = expect_status(
        ctx.put(
            "/api/maps/active",
            &dispatcher.token,
            json!({ "id": "map" }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(switched["id"], "map");
    assert_eq!(ctx.active_id(), "map");
    assert_eq!(
        ctx.redis_get("map:active").as_deref(),
        Some("map"),
        "the selection should be stored in Redis"
    );

    let active = expect_status(
        ctx.get("/api/maps/active", &dispatcher.token).await,
        StatusCode::OK,
    );
    assert_eq!(active["id"], "map");

    expect_status(
        ctx.put(
            "/api/maps/active",
            &dispatcher.token,
            json!({ "id": "   " }),
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
    expect_status(
        ctx.put(
            "/api/maps/active",
            &dispatcher.token,
            json!({ "id": "missing-map" }),
        )
        .await,
        StatusCode::NOT_FOUND,
    );

    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    expect_status(
        ctx.put("/api/maps/active", &engineer.token, json!({ "id": "map2" }))
            .await,
        StatusCode::FORBIDDEN,
    );

    // Return the shared state to the map other tests assume.
    expect_status(
        ctx.put(
            "/api/maps/active",
            &dispatcher.token,
            json!({ "id": "map2" }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(ctx.active_id(), "map2");
}

#[tokio::test]
async fn calculates_routes_and_rejects_out_of_bounds_points() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();

    let body = expect_status(
        ctx.post(
            "/api/getRoute",
            &dispatcher.token,
            json!({ "start_point": [5, 5], "end_point": [15, 15] }),
        )
        .await,
        StatusCode::OK,
    );
    let route = body["route"].as_array().unwrap();
    assert!(route.len() > 1);
    assert_eq!(body["distance"], route.len());

    expect_status(
        ctx.post(
            "/api/getRoute",
            &dispatcher.token,
            json!({ "start_point": [5, 5], "end_point": [100, 100] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    expect_status(
        ctx.post(
            "/api/getRoute",
            &engineer.token,
            json!({ "start_point": [1, 1], "end_point": [2, 2] }),
        )
        .await,
        StatusCode::OK,
    );
}

#[tokio::test]
async fn classifies_text_and_retrains_the_model() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();

    let body = expect_status(
        ctx.post(
            "/api/classify",
            &dispatcher.token,
            json!({ "text": "уход гироплатформы ИНС, курс смещается" }),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(body["engineer_type"], "avionics_engineer");
    assert!(body["confidence"].as_f64().unwrap() > 0.0);

    expect_status(
        ctx.post("/api/classify", &dispatcher.token, json!({ "text": "   " }))
            .await,
        StatusCode::BAD_REQUEST,
    );

    let trained = expect_status(
        ctx.post("/api/classify/train", &dispatcher.token, json!(null))
            .await,
        StatusCode::OK,
    );
    assert!(trained["documents"].as_u64().unwrap() > 0);
    assert!(trained["trained_at"].is_string());

    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    expect_status(
        ctx.post("/api/classify/train", &engineer.token, json!(null))
            .await,
        StatusCode::FORBIDDEN,
    );
}
