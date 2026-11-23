use crate::{
    database::{establish_pg_connection, establish_redis_connection},
    logging::init_logger,
    router::{
        arrive_aircraft, assign_engineer, classify_problem, complete_current_task,
        current_task_aog, current_task_aog_post, depart_aircraft, get_active_map, get_current_task,
        get_route, list_aircraft, list_aircraft_movements, list_aog, list_maps, set_active_map,
        train_classifier,
    },
    types::{
        config::{AppConfig, AppState, Args},
        doc::ApiDoc,
    },
};
use clap::Parser;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use std::{process, sync::Arc, time::Duration};
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
mod graph;
mod journal;
mod location;
mod logging;
mod maps;
mod middleware;
mod models;
mod parser;
mod rank;
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

    tracing::info!("Loading map catalog from {}...", args.map_path);
    let (catalog, cli_map_id) = match maps::load_catalog(&args.map_path) {
        Ok(loaded) => {
            tracing::info!(maps = loaded.0.entries.len(), "map catalog loaded");
            loaded
        }
        Err(e) => {
            tracing::error!("Error during parsing maps: {e:#}");
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

    let classifier =
        match crate::types::classifier::ProblemClassifier::train_from_dataset(&args.dataset_path) {
            Ok(model) => std::sync::Arc::new(std::sync::RwLock::new(model)),
            Err(e) => {
                tracing::error!("failed to train problem classifier: {e:#}");
                process::exit(1);
            }
        };

    let catalog_for_active = catalog.clone();
    let redis_for_active = redis_pool.clone();
    let active = match tokio::task::spawn_blocking(move || {
        let mut conn = redis_for_active.get()?;
        maps::resolve_active(&mut conn, &catalog_for_active, &cli_map_id)
    })
    .await
    {
        Ok(Ok(active)) => {
            tracing::info!(map_id = %active.id, "active map selected");
            std::sync::Arc::new(std::sync::RwLock::new(active))
        }
        Ok(Err(e)) => {
            tracing::error!("failed to select the active map: {e:#}");
            process::exit(1);
        }
        Err(e) => {
            tracing::error!("active map task join error: {e}");
            process::exit(1);
        }
    };

    // global application state
    let app_state = Arc::new(AppState {
        catalog,
        active,
        db_pool,
        redis_pool,
        jwt_secret: app_config.jwt_secret.clone(),
        dataset_path: std::path::PathBuf::from(&args.dataset_path),
        classifier,
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
        .route("/maps", get(list_maps))
        .route("/maps/active", get(get_active_map).put(set_active_map))
        .route("/getRoute", post(get_route))
        .route("/assign", post(assign_engineer))
        .route("/requests", post(router::ops::create_request))
        .route("/requests/{id}/queue", post(router::ops::queue_request))
        .route(
            "/requests/{id}/reassign",
            post(router::ops::reassign_request),
        )
        .route("/alerts", get(router::ops::list_alerts))
        .route("/defects", post(router::ops::create_defect))
        .route("/journal.csv", get(router::ops::export_journal))
        .route("/tasks/{id}/route", post(router::ops::refresh_route))
        .route("/tasks/current/accept", post(router::ops::accept_current))
        .route("/tasks/current/reject", post(router::ops::reject_current))
        .route("/tasks/current/tools", post(router::ops::pick_tools))
        .route("/tasks/current/mel", post(router::ops::set_mel))
        .route(
            "/tasks/current/waiting-part",
            post(router::ops::wait_for_part),
        )
        .route("/aircraft", get(list_aircraft))
        .route("/aircraft/arrive", post(arrive_aircraft))
        .route("/aircraft/depart", post(depart_aircraft))
        .route("/aircraft/movements", get(list_aircraft_movements))
        .route("/tasks/current", get(get_current_task))
        .route(
            "/tasks/current/aog",
            post(current_task_aog_post).put(current_task_aog),
        )
        .route("/tasks/current/complete", post(complete_current_task))
        .route("/tasks/aog", get(list_aog))
        .route("/classify", post(classify_problem))
        .route("/classify/train", post(train_classifier))
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
        .route("/activeEngineers", get(router::geo::active_engineers))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&app_state),
            middleware::auth_middleware,
        ));

    // global API router
    let api_routes = Router::new()
        .route("/v1/track", post(router::track::ingest))
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
