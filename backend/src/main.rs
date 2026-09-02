use crate::types::api::ApiDoc;
use std::{collections::HashSet, sync::Arc, time::Duration};
use utoipa::OpenApi;

use axum::{Json, Router, http::StatusCode, routing::get};
use tower_http::{
    compression::CompressionLayer,
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa_swagger_ui::SwaggerUi;

use crate::{router::get_map, types::map::Point};

mod bfs;
mod error;
mod parser;
mod router;
mod types;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "aeroflot-snippets=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // let map = parser::parse_map::<i64>("./assets/map.tmj", None);

    let map = parser::parse_from_json::<i64>("./assets/parsed/map.json");

    let map = match map {
        Ok(m) => m,
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    };

    let roads: HashSet<i64> = HashSet::from([
        2684354912, 2684355023, 2684355024, 2684354967, 2684354886, 3221225935, 3221225936,
        3221225879, 3221225798, 1610613200, 1610613199, 1610613143, 29,
    ]);

    let route = bfs::find_nearest::<i64>(&map, Point::new(0, 0), 407, &roads);

    let shared_map = Arc::new(map);

    let api_routes = Router::new()
        .route("/map", get(get_map))
        .route("/route", get(async || Json(route)))
        .with_state(shared_map);

    let app = Router::new()
        .nest("/api", api_routes)
        .merge(SwaggerUi::new("/swagger").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(10),
        ))
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001")
        .await
        .unwrap();

    _ = axum::serve(listener, app).await;
}
