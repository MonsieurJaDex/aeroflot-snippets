use std::{collections::HashSet, error::Error, io, sync::Arc};

use axum::{Json, Router, routing::get};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::router::get_map;

mod bfs;
mod error;
mod parser;
mod router;
mod types;

#[tokio::main]
async fn main() {
    match dotenvy::dotenv() {
        Ok(p) => println!("{}", p.to_str().unwrap()),
        Err(_) => {
            println!(".env file was not found in this directory or at parents");
            std::process::exit(1);
        }
    }

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

    let route = bfs::find_nearest::<i64>(&map, types::Point::new(0, 0), 407, &roads);

    let shared_map = Arc::new(map);

    let api_routes = Router::new()
        .route("/map", get(get_map))
        .route("/route", get(async || Json(route)))
        .with_state(shared_map);

    let app = Router::new()
        .nest("/api", api_routes)
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001")
        .await
        .unwrap();

    _ = axum::serve(listener, app).await;
}
