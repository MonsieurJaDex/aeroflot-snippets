use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use redis::TypedCommands;
use uuid::Uuid;

use crate::types::{auth::AccessClaims, config::AppState, enums::UserRole};
use crate::types::enums::TokenType;

#[derive(Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub role: UserRole,
}

pub async fn auth_middleware(
    State(app_state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = match req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        Some(token) if !token.is_empty() => token,
        _ => return (StatusCode::UNAUTHORIZED, "missing bearer access token").into_response(),
    };

    let claims = match decode_access_token(token, app_state.jwt_secret.as_bytes()) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = %e, "access token failed jwt validation");
            return (StatusCode::UNAUTHORIZED, "invalid or expired access token").into_response();
        }
    };

    let redis_pool = app_state.redis_pool.clone();
    let token_key = format!("access:{}", token);
    let lookup = tokio::task::spawn_blocking(move || {
        let mut conn = redis_pool.get()?;
        let id: Option<String> = conn.get(&token_key)?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(id)
    })
        .await;

    let stored_user_id = match lookup {
        Ok(Ok(Some(id))) => id,
        Ok(Ok(None)) => {
            return (
                StatusCode::UNAUTHORIZED,
                "access token was revoked, please login again",
            )
                .into_response();
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "error checking access token in redis");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "redis lookup task failed");
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

pub(crate) fn decode_access_token(
    token: &str,
    secret: &[u8],
) -> Result<AccessClaims, jsonwebtoken::errors::Error> {
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.set_required_spec_claims(&["exp", "sub"]);
    let data = jsonwebtoken::decode::<AccessClaims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret),
        &validation,
    )?;
    if data.claims.token_type != TokenType::Access {
        return Err(jsonwebtoken::errors::Error::from(
            jsonwebtoken::errors::ErrorKind::InvalidToken,
        ));
    }
    Ok(data.claims)
}