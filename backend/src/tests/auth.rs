//! Authentication endpoints: registration, login, token rotation, profiles.

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;

use crate::tests::support::{
    PASSWORD, TestContext, dispatcher_registration, engineer_registration, expect_status,
};
use crate::types::enums::EngineerType;
use crate::utils::verify_password;

#[tokio::test]
async fn registers_dispatcher_and_persists_password_hash() {
    let ctx = TestContext::new().await;
    let email = "dispatcher@test.example";

    let body = expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/register",
            None,
            Some(dispatcher_registration(email)),
        )
        .await,
        StatusCode::OK,
    );

    assert_eq!(body["email"], email);
    assert_eq!(body["user_role"], "Dispatcher");
    assert!(!body["access_token"].as_str().unwrap().is_empty());
    assert!(!body["refresh_token"].as_str().unwrap().is_empty());

    let id: Uuid = body["id"].as_str().unwrap().parse().unwrap();
    let row = ctx.dispatcher_row(id).expect("dispatcher row should exist");
    assert_eq!(row.email, email);
    assert_ne!(
        row.password_hash, PASSWORD,
        "password must not be stored in clear text"
    );
    verify_password(PASSWORD, &row.password_hash).expect("stored hash should verify");
    assert_eq!(ctx.dispatcher_count(), 1);

    let me = expect_status(
        ctx.get("/api/auth/me", body["access_token"].as_str().unwrap())
            .await,
        StatusCode::OK,
    );
    assert_eq!(me["name"], "Integration Dispatcher");
}

#[tokio::test]
async fn registers_engineer_with_specialization() {
    let ctx = TestContext::new().await;
    let email = "engineer@test.example";

    let body = expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/register",
            None,
            Some(engineer_registration(email, "avionics_engineer")),
        )
        .await,
        StatusCode::OK,
    );

    let id: Uuid = body["id"].as_str().unwrap().parse().unwrap();
    let row = ctx.engineer_row(id).expect("engineer row should exist");
    assert_eq!(row.email, email);
    assert_eq!(row.engineer_type, EngineerType::AvionicsEngineer);
    assert_eq!(ctx.engineer_count(), 1);

    let me = expect_status(
        ctx.get("/api/auth/me", body["access_token"].as_str().unwrap())
            .await,
        StatusCode::OK,
    );
    assert_eq!(me["user_role"], "Engineer");
    assert_eq!(me["engineer_type"], "avionics_engineer");
}

#[tokio::test]
async fn rejects_invalid_registration_payloads() {
    let ctx = TestContext::new().await;

    let invalid_email = expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/register",
            None,
            Some(dispatcher_registration("not-an-email")),
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
    assert_eq!(invalid_email["code"], "VALIDATION_ERROR");

    let missing_specialization = expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/register",
            None,
            Some(json!({
                "email": "engineer@test.example",
                "name": "No Specialization",
                "password": PASSWORD,
                "user_role": "Engineer",
            })),
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
    assert_eq!(missing_specialization["code"], "VALIDATION_ERROR");
    assert_eq!(ctx.dispatcher_count(), 0);
    assert_eq!(ctx.engineer_count(), 0);
}

#[tokio::test]
async fn rejects_duplicate_email_across_both_tables() {
    let ctx = TestContext::new().await;
    let email = "shared@test.example";

    expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/register",
            None,
            Some(engineer_registration(email, "fueling_crew")),
        )
        .await,
        StatusCode::OK,
    );
    expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/register",
            None,
            Some(dispatcher_registration(email)),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    assert_eq!(ctx.engineer_count(), 1);
    assert_eq!(ctx.dispatcher_count(), 0);
}

#[tokio::test]
async fn logs_in_and_rejects_wrong_credentials() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();

    let body = expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/login",
            None,
            Some(json!({ "email": dispatcher.email, "password": PASSWORD })),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(body["id"], dispatcher.id.to_string());
    assert_eq!(body["user_role"], "Dispatcher");
    assert!(!body["access_token"].as_str().unwrap().is_empty());

    let wrong = expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/login",
            None,
            Some(json!({ "email": dispatcher.email, "password": "wrong-password" })),
        )
        .await,
        StatusCode::UNAUTHORIZED,
    );
    assert_eq!(wrong["code"], "INVALID_CREDENTIALS");

    expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/login",
            None,
            Some(json!({ "email": "nobody@test.example", "password": PASSWORD })),
        )
        .await,
        StatusCode::UNAUTHORIZED,
    );
}

#[tokio::test]
async fn rotates_refresh_token_and_invalidates_the_old_one() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();

    let body = expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/update_access",
            None,
            Some(json!({ "refresh_token": dispatcher.refresh_token })),
        )
        .await,
        StatusCode::OK,
    );
    assert!(!body["access_token"].as_str().unwrap().is_empty());
    assert_ne!(body["refresh_token"], dispatcher.refresh_token);

    expect_status(
        ctx.request(
            axum::http::Method::POST,
            "/api/auth/update_access",
            None,
            Some(json!({ "refresh_token": dispatcher.refresh_token })),
        )
        .await,
        StatusCode::UNAUTHORIZED,
    );
}

#[tokio::test]
async fn returns_engineer_name_and_validates_path() {
    let ctx = TestContext::new().await;
    let engineer = ctx.engineer(EngineerType::AvionicsEngineer);

    let body = expect_status(
        ctx.get(
            &format!("/api/auth/getEngineerName/{}", engineer.id),
            &engineer.token,
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(body["name"], "Test Engineer");

    expect_status(
        ctx.get("/api/auth/getEngineerName/not-a-uuid", &engineer.token)
            .await,
        StatusCode::BAD_REQUEST,
    );
    expect_status(
        ctx.get(
            &format!("/api/auth/getEngineerName/{}", Uuid::new_v4()),
            &engineer.token,
        )
        .await,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn me_requires_a_valid_token() {
    let ctx = TestContext::new().await;
    let dispatcher = ctx.dispatcher();

    let unauthorized = expect_status(
        ctx.request(axum::http::Method::GET, "/api/auth/me", None, None)
            .await,
        StatusCode::UNAUTHORIZED,
    );
    assert_eq!(unauthorized["code"], "UNAUTHORIZED");

    expect_status(
        ctx.request(
            axum::http::Method::GET,
            "/api/auth/me",
            Some("garbage-token"),
            None,
        )
        .await,
        StatusCode::UNAUTHORIZED,
    );

    let me = expect_status(
        ctx.get("/api/auth/me", &dispatcher.token).await,
        StatusCode::OK,
    );
    assert_eq!(me["id"], dispatcher.id.to_string());
    assert!(me["engineer_type"].is_null());
}
