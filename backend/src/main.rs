use crate::{
    database::establish_connection,
    router::get_route,
    types::{
        config::{AppConfig, AppState},
        doc::ApiDoc,
    },
};
use std::{collections::HashSet, process, sync::Arc, time::Duration};
use utoipa::OpenApi;

use axum::{
    Router,
    http::StatusCode,
    routing::{get, post},
};
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

mod database;
mod models;
mod parser;
mod router;
mod search;
mod types;

#[tokio::main]
async fn main() {
    // load configuration
    let app_config: Arc<AppConfig> = match AppConfig::new() {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            tracing::error!("Error during app configuration loading: {}", e);
            process::exit(-1);
        }
    };

    // TODO: use debug for logging level
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "aeroflot_snippets=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Loading map from JSON...");

    let (map, roads) = match parser::parse_from_json("./assets/map.json") {
        Ok(jm) => {
            let map = jm.map;
            let roads: HashSet<i64> = jm.road.into_iter().collect();
            tracing::info!("Map loaded");
            (map, roads)
        }
        Err(e) => {
            tracing::error!("Error during parsing map: {}", e.to_string());
            process::exit(1);
        }
    };

    let _route = search::find_nearest(&map, Point::new(0, 0), 466, &roads);

    tracing::info!("Attempting to establish database connection...");
    let db_pool = match establish_connection(&app_config.database_url) {
        Ok(pool) => {
            tracing::info!("Database connection successful. Pool initialized");
            pool
        }
        Err(e) => {
            tracing::error!("Error during database pool initialization: {}", e);
            process::exit(1);
        }
    };

    let app_state = Arc::new(AppState {
        road_points: roads,
        map: map,
        db_pool: db_pool,
    });

    let api_routes = Router::new()
        .route("/map", get(get_map))
        .route("/getRoute", post(get_route))
        .with_state(Arc::clone(&app_state));

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

    let server_addr = format!("{}:{}", &app_config.host, &app_config.port);

    let listener = tokio::net::TcpListener::bind(&server_addr).await.unwrap();
    tracing::info!("Running sever at: http://{server_addr}");

    _ = axum::serve(listener, app).await;
}
