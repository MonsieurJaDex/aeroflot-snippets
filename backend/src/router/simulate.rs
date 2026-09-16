use std::{str::FromStr, sync::Arc};

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Response, StatusCode},
    response::IntoResponse,
};
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use redis::TypedCommands;
use serde_json::json;
use uuid::Uuid;

use crate::{
    database::schema,
    types::{
        config::AppState,
        dto::{UpdateEngineerPositionRequest, UpdateTransportPositionRequest},
        enums::SpecialVehicle,
        map::Point,
    },
};

// add possibility to move engineer and control simulation params from API (speed etc.)
#[utoipa::path(
    post,
    path="/api/simulate/update_engineer_position",
    description="Manual engineer position update",
    request_body=UpdateEngineerPositionRequest,
    responses(
        (status=200, description="Successful update", body=String),
        (status=400, description="Invalid request format", body=String),
        (status=500, description="Server-side error", body=String)
    )
)]
pub async fn update_engineer_position_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<UpdateEngineerPositionRequest>,
) -> Response<Body> {
    let uuid: Uuid = match Uuid::parse_str(&payload.id) {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "invalid ID format, parsing faile").into_response();
        }
    };

    if payload.new_point.1 < 0 || payload.new_point.1 as usize > app_state.map.0.len() {
        return (StatusCode::BAD_REQUEST, "invalid Y point coordinate").into_response();
    }

    if let Some(row) = app_state.map.0.get(0) {
        if payload.new_point.0 < 0 || payload.new_point.0 as usize > row.len() {
            return (StatusCode::BAD_REQUEST, "invalid X point coordinate").into_response();
        }
    }

    let mut redis_conn = match app_state.redis_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    match redis_conn.set(format!("position:{}", uuid), payload.new_point.as_value()) {
        Ok(_) => (),
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    StatusCode::OK.into_response()
}

#[utoipa::path(
    get,
    path="/api/simulate/active_engineers",
    description="Get all active engineers",
    responses(
        (status=200, description="Successful fetch", body=Vec<String>),
        (status=500, description="Server-side error", body=String)
    )
)]
pub async fn active_engineers(State(app_state): State<Arc<AppState>>) -> Response<Body> {
    let mut pg_conn = match app_state.db_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting postgres connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let ids: Vec<Uuid> = match schema::tasks::table
        .filter(schema::tasks::is_active.eq(true))
        .select(schema::tasks::assigned_engineer)
        .load(&mut pg_conn)
    {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(error = %e, "error happened during gathering all engineers");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let ids: Vec<String> = ids.iter().map(|u| u.to_string()).collect();

    (StatusCode::OK, Json(json!({"active_engineers": ids}))).into_response()
}

#[utoipa::path(
    post,
    path="/api/simulate/update_transport_position",
    description="Manual special vehicle position update (creates the vehicle in redis if it did not exist yet)",
    request_body=UpdateTransportPositionRequest,
    responses(
        (status=200, description="Successful update", body=String),
        (status=400, description="Invalid request format", body=String),
        (status=500, description="Server-side error", body=String)
    )
)]
pub async fn update_transport_position_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<UpdateTransportPositionRequest>,
) -> Response<Body> {
    let uuid: Uuid = match Uuid::parse_str(&payload.id) {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "invalid ID format, parsing failed").into_response();
        }
    };

    if payload.new_point.1 < 0 || payload.new_point.1 as usize > app_state.map.0.len() {
        return (StatusCode::BAD_REQUEST, "invalid Y point coordinate").into_response();
    }

    if let Some(row) = app_state.map.0.get(0) {
        if payload.new_point.0 < 0 || payload.new_point.0 as usize > row.len() {
            return (StatusCode::BAD_REQUEST, "invalid X point coordinate").into_response();
        }
    }

    let mut redis_conn = match app_state.redis_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // ключ вида position:{vehicle_type}:{uuid} — то же соглашение, что уже
    // используется в assign_engineer при поиске техники через scan_match
    let key = format!("position:{}:{}", payload.vehicle_type, uuid);

    match redis_conn.set(key, payload.new_point.as_value()) {
        Ok(_) => (),
        Err(e) => {
            tracing::error!(error = %e, "Error during setting transport position in redis");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    StatusCode::OK.into_response()
}

#[utoipa::path(
    get,
    path="/api/simulate/active_engineers",
    description="Get all active engineers",
    responses(
        (status=200, description="Successful fetch", body=Vec<String>),
        (status=500, description="Server-side error", body=String)
    )
)]
pub async fn active_engineers(State(app_state): State<Arc<AppState>>) -> Response<Body> {
    let mut pg_conn = match app_state.db_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting postgres connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let ids: Vec<Uuid> = match schema::tasks::table
        .filter(schema::tasks::is_active.eq(true))
        .select(schema::tasks::assigned_engineer)
        .load(&mut pg_conn)
    {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(error = %e, "error happened during gathering all engineers");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let ids: Vec<String> = ids.iter().map(|u| u.to_string()).collect();

    (StatusCode::OK, Json(json!({"active_engineers": ids}))).into_response()
}

#[utoipa::path(
    post,
    path="/api/simulate/get_engineers_positions",
    description="Get all engineers with their positions",
    responses(
        (status=200, description="Successful fetch", body=String),
        (status=500, description="Server-side error", body=String)
    )
)]
pub async fn get_engineers_positions(State(app_state): State<Arc<AppState>>) -> Response<Body> {
    let mut pg_conn = match app_state.db_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting postgres connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut redis_conn = match app_state.redis_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let ids: Vec<Uuid> = match schema::engineers::table
        .select(schema::engineers::id)
        .load(&mut pg_conn)
    {
        Err(e) => {
            tracing::error!(error = %e, "error happened during gathering all engineers");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        Ok(u) => u,
    };

    let mut pos = Vec::new();
    for id in &ids {
        match redis_conn.get(format!("position:{}", id)) {
            Ok(r) => match r {
                Some(res) => pos.push(Some(Point::from_value(res).unwrap())),
                None => pos.push(None),
            },
            Err(e) => {
                tracing::error!(error = %e, "error happened during position extracting");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };
    }

    let response: Vec<(&Uuid, Option<Point>)> = ids.iter().zip(pos).map(|(i, p)| (i, p)).collect();

    Json(response).into_response()
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct TransportPositionEntry {
    pub id: Uuid,
    pub vehicle_type: SpecialVehicle,
    pub point: Point,
}

#[utoipa::path(
    get,
    path="/api/simulate/get_transport_positions",
    description="Get all special vehicles with their type, uuid and position",
    responses(
        (status=200, description="Successful fetch", body=Vec<TransportPositionEntry>),
        (status=500, description="Server-side error", body=String)
    )
)]
pub async fn get_transport_positions(State(app_state): State<Arc<AppState>>) -> Response<Body> {
    let mut redis_conn = match app_state.redis_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Транспорт не хранится в Postgres (в отличие от инженеров) — единственный
    // источник истины про весь существующий транспорт это сам Redis, поэтому
    // перечисляем ключи через SCAN, а не через выборку из БД.
    let keys: Vec<String> = {
        let iter: redis::Iter<'_, String> = match redis_conn.scan_match("position:*:*") {
            Ok(iter) => iter,
            Err(e) => {
                tracing::error!(error = %e, "error happened during scanning transport keys");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };
        iter.filter_map(|k| k.ok()).collect()
    };

    let mut result: Vec<TransportPositionEntry> = Vec::with_capacity(keys.len());

    for key in keys {
        // ключ вида position:{vehicle_type}:{uuid}
        let mut parts = key.splitn(3, ':');
        let _prefix = parts.next(); // "position"
        let type_str = match parts.next() {
            Some(t) => t,
            None => {
                tracing::warn!(key = %key, "malformed transport key, skipped");
                continue;
            }
        };
        let uuid_str = match parts.next() {
            Some(u) => u,
            None => {
                tracing::warn!(key = %key, "malformed transport key, skipped");
                continue;
            }
        };

        let vehicle_type = match SpecialVehicle::try_from(type_str) {
            Ok(v) => v,
            Err(_) => {
                tracing::warn!(key = %key, type_str = %type_str, "unknown vehicle type in key, skipped");
                continue;
            }
        };

        let id = match Uuid::from_str(uuid_str) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(error = %e, key = %key, "malformed transport uuid, skipped");
                continue;
            }
        };

        let point = match redis_conn.get(&key) {
            Ok(Some(v)) => match Point::from_value(v) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, key = %key, "malformed transport position, skipped");
                    continue;
                }
            },
            Ok(None) => continue, // ключ исчез между SCAN и GET — редко, но возможно
            Err(e) => {
                tracing::error!(error = %e, key = %key, "error happened during transport position fetching");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

        result.push(TransportPositionEntry {
            id,
            vehicle_type,
            point,
        });
    }

    Json(result).into_response()
}
