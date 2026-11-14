use crate::{
    database::{establish_pg_connection, establish_redis_connection},
    logging::init_logger,
    router::{
        assign_engineer, complete_current_task, current_task_aog, get_current_task, get_route,
        list_aircraft, list_aog,
    },
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

// migration folder initializing
const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");

#[tokio::main]
async fn main() {
    let app_config: Arc<AppConfig> = match AppConfig::new() {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            eprintln!("Error during app configuration loading: {}", e);
            process::exit(1);
        }
    };

    init_logger(app_config.debug);

    // loading cli arguments
    let args = Args::parse();
    if !args.validate_path() {
        tracing::error!("Provided map file was not found: {}", args.map_path);
        process::exit(1);
    }

    // loading map
    tracing::info!("Loading map from JSON...");

    // parsing map and roads from json config file
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

    // initializing database and redis connection pools
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

    // applying database migrations from diesel

    let migrations_pool = db_pool.clone();
    let migrate_result = tokio::task::spawn_blocking(move || {
        let mut conn = migrations_pool.get()?;
        conn.run_pending_migrations(MIGRATIONS).map(|_| ())
    })
    .await;

    match migrate_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::error!("migrations failed: {}", e);
            process::exit(1);
        }
        Err(e) => {
            tracing::error!("migrations task join error: {}", e);
            process::exit(1);
        }
    }

    // application API layer

    // global application state
    let app_state = Arc::new(AppState {
        road_points: roads,
        map,
        db_pool,
        redis_pool,
        jwt_secret: app_config.jwt_secret.clone(),
    });

    // authentication and authorization router
    let auth_public = Router::new()
        .route("/register", post(router::auth::register_handler))
        .route("/login", post(router::auth::login_handler))
        .route("/update_access", post(router::auth::update_access_token))
        .route(
            "/getEngineerName/{engineer_uuid}",
            get(router::auth::get_engineer_name_handler),
        );
    let auth_me = Router::new()
        .route("/me", get(router::auth::me_handler))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&app_state),
            middleware::auth_middleware,
        ));
    let auth_router = auth_public.merge(auth_me);

    // routes that protected with required bearer auth header
    let protected_routes = Router::new()
        .route("/map", get(get_map))
        .route("/getRoute", post(get_route))
        .route("/assign", post(assign_engineer))
        .route("/aircraft", get(list_aircraft))
        .route("/tasks/current", get(get_current_task))
        .route("/tasks/current/aog", post(current_task_aog))
        .route("/tasks/current/complete", post(complete_current_task))
        .route("/tasks/aog", get(list_aog))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&app_state),
            middleware::auth_middleware,
        ));

    // routes of geo functionality
    let geo_routes = Router::new()
        .route(
            "/updateEngineerPosition",
            post(router::geo::update_engineer_position_handler),
        )
        .route(
            "/updateTransportPosition",
            post(router::geo::update_transport_position_handler),
        )
        .route(
            "/getEngineersPositions",
            get(router::geo::get_engineers_positions),
        )
        .route(
            "/getTransportPositions",
            get(router::geo::get_transport_positions),
        )
        .route("/activeEngineers", get(router::geo::active_engineers));

    // global API router
    let api_routes = Router::new()
        .nest("/auth", auth_router)
        .nest("/geo", geo_routes)
        .merge(protected_routes)
        .with_state(Arc::clone(&app_state));

    // application router
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

    // server start
    let server_addr = format!("{}:{}", &app_config.host, &app_config.port);

    let listener = match tokio::net::TcpListener::bind(&server_addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("failed to bind {}: {}", server_addr, e);
            process::exit(1);
        }
    };
    tracing::info!("Running server at: http://{server_addr}");

    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!("server error: {}", e);
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install ctrl_c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
