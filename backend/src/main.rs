use crate::{
    database::{establish_pg_connection, establish_redis_connection},
    logging::init_logger,
    router::{assign_engineer, get_route},
    types::{
        config::{AppConfig, AppState, Args},
        doc::ApiDoc,
    },
};
use clap::Parser;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use std::{collections::HashSet, process, sync::Arc, time::Duration};
use utoipa::OpenApi;

use axum::{
    Router,
    http::StatusCode,
    routing::{get, post},
};
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use utoipa_swagger_ui::SwaggerUi;

use crate::router::get_map;

mod database;
mod logging;
mod middleware;
mod models;
mod parser;
mod router;
mod search;
mod types;
mod utils;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");

#[tokio::main]
async fn main() {
    // load configuration
    let app_config: Arc<AppConfig> = match AppConfig::new() {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            tracing::error!("Error during app configuration loading: {}", e);
            process::exit(1);
        }
    };

    // load logger
    init_logger(app_config.debug);

    // loading cli arguments
    let args = Args::parse();
    if !args.validate_path() {
        tracing::error!("Provided map file was not found: {}", args.map_path);
        process::exit(1);
    }

    // loading map
    tracing::info!("Loading map from JSON...");

    let (map, roads) = match parser::parse_from_json(&args.map_path) {
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

    tracing::info!("Attempting to establish database connections...");

    let (db_pool, redis_pool) = match tokio::try_join!(
        establish_pg_connection(&app_config.database_url),
        establish_redis_connection(&app_config.redis_url)
    ) {
        Ok((pg, rd)) => {
            tracing::info!("Database connections successful. Pools initialized");
            (pg, rd)
        }
        Err(e) => {
            tracing::error!("Error during database pool initialization: {}", e);
            process::exit(1);
        }
    };

    // run database migrations from diesel

    match db_pool.get() {
        Ok(mut pool) => match pool.run_pending_migrations(MIGRATIONS) {
            Ok(_) => (),
            Err(e) => {
                tracing::error!(
                    "Error during database pool using for migrations pending: {}",
                    e
                );
                process::exit(1);
            }
        },
        Err(e) => {
            tracing::error!(
                "Error during database pool using for migrations pending: {}",
                e
            );
            process::exit(1);
        }
    }

    // application API layer

    let app_state = Arc::new(AppState {
        road_points: roads,
        map: map,
        db_pool: db_pool,
        redis_pool: redis_pool,
        jwt_secret: app_config.jwt_secret.clone(),
    });

    let auth_router = Router::new()
        .route("/register", post(router::auth::register_handler))
        .route("/login", post(router::auth::login_handler))
        .route("/update_access", post(router::auth::update_access_token))
        .route(
            "/get_engineer_name",
            post(router::auth::get_engineer_name_handler),
        );

    let protected_routes = Router::new()
        .route("/map", get(get_map))
        .route("/getRoute", post(get_route))
        .route("/assign", post(assign_engineer))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&app_state),
            middleware::auth_middleware,
        ));

    let simulate_routes = Router::new()
        .route(
            "/update_engineer_position",
            post(router::simulate::update_engineer_position_handler),
        )
        .route(
            "/update_transport_position",
            post(router::simulate::update_transport_position_handler),
        )
        .route(
            "/get_engineers_positions",
            get(router::simulate::get_engineers_positions),
        )
        .route(
            "/get_transport_positions",
            get(router::simulate::get_transport_positions),
        )
        .route("/active_engineers", get(router::simulate::active_engineers))
        .with_state(Arc::clone(&app_state));

    let api_routes = Router::new()
        .nest("/auth", auth_router)
        .nest("/simulate", simulate_routes)
        .merge(protected_routes)
        .with_state(Arc::clone(&app_state));

    let app = Router::new()
        .nest("/api", api_routes)
        .merge(SwaggerUi::new("/swagger").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
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

    // TODO:
    // make simulated.rs, add container
    // separate routers to routers module fully
    // add comments

    _ = axum::serve(listener, app).await;
}
