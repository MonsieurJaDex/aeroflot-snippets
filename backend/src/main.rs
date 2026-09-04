use crate::{
    database::establish_connection,
    models::engineer::EngineerRow,
    router::get_route,
    types::{
        config::{AppConfig, AppState},
        doc::ApiDoc,
    },
};
use diesel::{Expression, QueryDsl, RunQueryDsl, SelectableHelper};
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
mod error;
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
            println!("Error during app configuration loading: {}", e);
            process::exit(-1);
        }
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "aeroflot-snippets=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let conn = &mut establish_connection(&app_config.database_url)
        .expect("Databse connection establishment failed");

    use self::database::schema::engineers::dsl::*;

    let results = engineers
        .select(EngineerRow::as_select())
        .load(conn)
        .unwrap();

    dbg!(results);

    // let map = parser::parse_map::<i64>("./assets/map.tmj", None);

    let map = parser::parse_from_json("./assets/parsed/map.json");

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

    let _route = search::find_nearest(&map, Point::new(0, 0), 466, &roads);

    let app_state = Arc::new(AppState {
        road_points: roads,
        map: map,
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

    let listener =
        tokio::net::TcpListener::bind(format!("{}:{}", &app_config.host, &app_config.port))
            .await
            .unwrap();

    _ = axum::serve(listener, app).await;
}
