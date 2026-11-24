//! Aircraft endpoints: arrival, departure, movement history and AOG cleanup.

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;

use crate::tests::support::{TestContext, expect_status};
use crate::types::enums::EngineerType;

fn position_key(ctx: &TestContext, aircraft_id: Uuid) -> String {
    format!("map:{}:position:aircraft:{aircraft_id}", ctx.active_id())
}

#[tokio::test]
async fn arranges_arrival_departure_and_movement_history() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let aircraft_id = Uuid::new_v4();

    let arrived = expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &dispatcher.token,
            json!({ "id": aircraft_id, "point": [10, 10] }),
        )
        .await,
        StatusCode::OK,
    );
    let movement_id: Uuid = arrived["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(arrived["aircraft_id"], aircraft_id.to_string());
    assert_eq!(arrived["map_id"], ctx.active_id());
    assert_eq!(arrived["point"], json!([10, 10]));
    assert!(arrived["departed_at"].is_null());

    let movement = ctx
        .movement(movement_id)
        .expect("movement row should be stored");
    assert_eq!(movement.aircraft_id, aircraft_id);
    assert_eq!(movement.map_id, ctx.active_id());
    assert_eq!((movement.plane_x, movement.plane_y), (10, 10));
    assert!(movement.departed_at.is_none());

    assert_eq!(
        ctx.redis_get(&position_key(&ctx, aircraft_id)),
        Some("10,10".to_owned()),
        "arrival should place the aircraft in Redis"
    );

    let aircraft = expect_status(
        ctx.get("/api/aircraft", &dispatcher.token).await,
        StatusCode::OK,
    );
    assert_eq!(aircraft["total"], 1);
    assert_eq!(aircraft["aircraft"][0]["id"], aircraft_id.to_string());
    assert!(aircraft["aircraft"][0]["arrived_at"].is_string());

    let movements = expect_status(
        ctx.get("/api/aircraft/movements", &dispatcher.token).await,
        StatusCode::OK,
    );
    assert_eq!(movements["total"], 1);
    assert_eq!(movements["movements"][0]["id"], movement_id.to_string());

    let departed = expect_status(
        ctx.post(
            "/api/aircraft/depart",
            &dispatcher.token,
            json!({ "id": aircraft_id }),
        )
        .await,
        StatusCode::OK,
    );
    assert!(departed["departed_at"].is_string());

    let closed = ctx
        .movement(movement_id)
        .expect("movement row should remain");
    assert!(closed.departed_at.is_some());
    assert!(
        ctx.redis_get(&position_key(&ctx, aircraft_id)).is_none(),
        "departure should remove the Redis position"
    );

    let aircraft = expect_status(
        ctx.get("/api/aircraft", &dispatcher.token).await,
        StatusCode::OK,
    );
    assert_eq!(aircraft["total"], 0);
    let movements = expect_status(
        ctx.get("/api/aircraft/movements", &dispatcher.token).await,
        StatusCode::OK,
    );
    assert_eq!(movements["total"], 1);
    assert!(movements["movements"][0]["departed_at"].is_string());
}

#[tokio::test]
async fn prevents_duplicate_arrivals_and_bad_positions() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();

    expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &dispatcher.token,
            json!({ "id": first, "point": [5, 5] }),
        )
        .await,
        StatusCode::OK,
    );

    let busy = expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &dispatcher.token,
            json!({ "id": second, "point": [5, 5] }),
        )
        .await,
        StatusCode::CONFLICT,
    );
    assert_eq!(busy["code"], "CELL_IS_BUSY");

    let duplicate = expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &dispatcher.token,
            json!({ "id": first, "point": [6, 6] }),
        )
        .await,
        StatusCode::CONFLICT,
    );
    assert_eq!(duplicate["code"], "AIRCRAFT_ALREADY_ACTIVE");

    expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &dispatcher.token,
            json!({ "id": Uuid::new_v4(), "point": [999, 999] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &dispatcher.token,
            json!({ "id": "not-a-uuid", "point": [1, 1] }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    let engineer = ctx.engineer(EngineerType::AviationTechnician);
    expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &engineer.token,
            json!({ "id": Uuid::new_v4(), "point": [1, 1] }),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
}

#[tokio::test]
async fn validates_departure_time_and_pagination() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();

    expect_status(
        ctx.post(
            "/api/aircraft/depart",
            &dispatcher.token,
            json!({ "id": Uuid::new_v4() }),
        )
        .await,
        StatusCode::NOT_FOUND,
    );

    let aircraft_id = Uuid::new_v4();
    expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &dispatcher.token,
            json!({
                "id": aircraft_id,
                "point": [3, 3],
                "arrived_at": "2026-11-24T12:00:00Z"
            }),
        )
        .await,
        StatusCode::OK,
    );
    expect_status(
        ctx.post(
            "/api/aircraft/depart",
            &dispatcher.token,
            json!({ "id": aircraft_id, "departed_at": "2026-11-24T11:00:00Z" }),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &dispatcher.token,
            json!({ "id": Uuid::new_v4(), "point": [4, 4] }),
        )
        .await,
        StatusCode::OK,
    );

    let page = expect_status(
        ctx.get("/api/aircraft?limit=1&offset=1", &dispatcher.token)
            .await,
        StatusCode::OK,
    );
    assert_eq!(page["total"], 2);
    assert_eq!(page["aircraft"].as_array().unwrap().len(), 1);

    expect_status(
        ctx.get("/api/aircraft?limit=0", &dispatcher.token).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
}

#[tokio::test]
async fn departure_clears_aog_flag() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();
    let aircraft_id = Uuid::new_v4();

    expect_status(
        ctx.post(
            "/api/aircraft/arrive",
            &dispatcher.token,
            json!({ "id": aircraft_id, "point": [7, 7] }),
        )
        .await,
        StatusCode::OK,
    );
    let aog_key = format!("map:{}:aog:{aircraft_id}", ctx.active_id());
    ctx.redis_set(&aog_key, "1");

    expect_status(
        ctx.post(
            "/api/aircraft/depart",
            &dispatcher.token,
            json!({ "id": aircraft_id }),
        )
        .await,
        StatusCode::OK,
    );
    assert!(
        !ctx.redis_exists(&aog_key),
        "departure should clear the AOG flag"
    );
}
