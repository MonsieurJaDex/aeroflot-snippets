use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use redis::TypedCommands;
use uuid::Uuid;

use crate::types::{auth::AccessClaims, config::AppState, enums::UserRole};

#[derive(Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub role: UserRole,
}

pub async fn auth_middleware(
    State(app_state): State<Arc<AppState>>,
    TypedHeader(Authorization(bearer)): TypedHeader<Authorization<Bearer>>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = bearer.token();

    let claims = match decode_access_token(token, app_state.jwt_secret.as_bytes()) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = %e, "access token failed jwt validation");
            return (StatusCode::UNAUTHORIZED, "invalid or expired access token").into_response();
        }
    };

    let mut redis_conn = match app_state.redis_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let stored_user_id = match redis_conn.get(format!("access:{}", token)) {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                "access token was revoked, please login again",
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "error checking access token in redis");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    match Uuid::parse_str(&stored_user_id) {
        Ok(id) if id == claims.sub => {}
        Ok(_) => {
            tracing::warn!(token_sub = %claims.sub, "redis user id mismatch with jwt sub");
            return (StatusCode::UNAUTHORIZED, "invalid access token").into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "corrupted user id stored in redis for access token");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    // Прокидываем аутентифицированного пользователя дальше по цепочке
    req.extensions_mut().insert(AuthUser {
        id: claims.sub,
        role: claims.role,
    });

    next.run(req).await
}

fn decode_access_token(
    token: &str,
    secret: &[u8],
) -> Result<AccessClaims, jsonwebtoken::errors::Error> {
    let data = jsonwebtoken::decode::<AccessClaims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret),
        &jsonwebtoken::Validation::default(),
    )?;
    Ok(data.claims)
}
