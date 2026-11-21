use std::sync::Arc;

use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use redis::TypedCommands;
use uuid::Uuid;

use crate::types::dto::ErrorResponse;
use crate::types::enums::TokenType;
use crate::types::{auth::AccessClaims, config::AppState, enums::UserRole};

fn auth_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            code: code.to_owned(),
            message: message.to_owned(),
            details: None,
            request_id: None,
        }),
    )
        .into_response()
}

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
        _ => {
            return auth_error(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Missing or invalid access token",
            );
        }
    };

    let claims = match decode_access_token(token, app_state.jwt_secret.as_bytes()) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = %e, "access token failed jwt validation");
            return auth_error(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Missing or invalid access token",
            );
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
            return auth_error(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Missing or invalid access token",
            );
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "error checking access token in redis");
            return auth_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "internal server error",
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "redis lookup task failed");
            return auth_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "internal server error",
            );
        }
    };

    match Uuid::parse_str(&stored_user_id) {
        Ok(id) if id == claims.sub => {}
        Ok(_) => {
            tracing::warn!(token_sub = %claims.sub, "redis user id mismatch with jwt sub");
            return auth_error(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Missing or invalid access token",
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "corrupted user id stored in redis for access token");
            return auth_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "internal server error",
            );
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