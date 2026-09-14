use std::{str::FromStr, sync::Arc};

use axum::{
    Json,
    body::Body,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl, SelectableHelper};
use redis::TypedCommands;
use uuid::Uuid;
use validator::Validate;

use crate::{
    database::schema,
    models::{dispatcher::Dispatcher, engineer::Engineer},
    types::{
        config::AppState,
        dto::{
            GetUserNameRequest, GetUserNameResponse, LoginRequest, LoginResponse, RegisterRequest,
            UpdateAccessRequest, UpdateAccessResponse,
        },
        enums::{AuthenticatedUser, UserRole},
    },
    utils::{
        create_token_pair, hash_password, revoke_and_store_access_token, revoke_and_store_tokens,
        verify_password,
    },
};

#[utoipa::path(
    post,
    path="/api/auth/register",
    description="Register new user profile",
    request_body=RegisterRequest,
    responses(
        (status=200, description="Successful registration", body=LoginResponse),
        (status=401, description="Register failed because of reques", body=String),
        (status=500, description="Server-side error happened", body=String)
    )
)]
pub async fn register_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> Response<Body> {
    if let Err(e) = payload.validate() {
        return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
    }

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

    let hashed_password = match hash_password(payload.password) {
        Ok(hash) => hash,
        Err(e) => {
            tracing::error!(error = %e, "Error during hashing password");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let new_id = Uuid::new_v4();

    match payload.user_role {
        UserRole::Dispatcher => {
            let new_dispatcher = Dispatcher {
                id: new_id,
                name: payload.name.clone(),
                email: payload.email,
                password_hash: hashed_password,
            };
            if let Err(_) = diesel::insert_into(schema::dispatchers::table)
                .values(&new_dispatcher)
                .execute(&mut pg_conn)
            {
                return (
                    StatusCode::BAD_REQUEST,
                    "dispatcher with that email is already exists",
                )
                    .into_response();
            }
        }
        UserRole::Engineer => {
            if payload.engineer_type.is_none() {
                return (
                    StatusCode::BAD_REQUEST,
                    "User role was engineer, but engineer_type is missing",
                )
                    .into_response();
            }

            let new_engineer = Engineer {
                id: new_id,
                email: payload.email.clone(),
                name: payload.name.clone(),
                password_hash: hashed_password,
                engineer_type: payload.engineer_type.unwrap(),
            };

            if let Err(_) = diesel::insert_into(schema::engineers::table)
                .values(&new_engineer)
                .execute(&mut pg_conn)
            {
                return (
                    StatusCode::BAD_REQUEST,
                    "engineer with this email is already exists",
                )
                    .into_response();
            }
        }
    };

    let (access, refresh) = match create_token_pair(
        new_id,
        payload.user_role.clone(),
        app_state.jwt_secret.as_bytes(),
    ) {
        Ok((a, r)) => (a, r),
        Err(e) => {
            tracing::error!(uuid = %&new_id, error = %e, "error during creating token pair for user");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    if let Err(e) = revoke_and_store_tokens(
        &mut redis_conn,
        new_id,
        &access,
        &refresh,
        &payload.user_role,
    ) {
        tracing::error!(error = %e, "error during storing tokens in redis");
        return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
    }

    Json(LoginResponse {
        name: payload.name.clone(),
        user_role: payload.user_role,
        access_token: access,
        refresh_token: refresh,
    })
    .into_response()
}

#[utoipa::path(
    post,
    path="/api/auth/get_engineer_name",
    description="Login into existing user profile and get refresh and access tokens",
    request_body=GetUserNameRequest,
    responses(
        (status=200, description="Successful fetch", body=LoginResponse),
        (status=401, description="Engineer was not found", body=String),
        (status=500, description="Server-side error happened", body=String)
    )
)]
pub async fn get_engineer_name_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<GetUserNameRequest>,
) -> Response<Body> {
    let uuid: Uuid = match Uuid::parse_str(&payload.id) {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "invalid ID format, parsing faile").into_response();
        }
    };

    let mut pg_conn = match app_state.db_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting postgres connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let name: Option<String> = match schema::engineers::table
        .filter(schema::engineers::id.eq(uuid))
        .select(schema::engineers::name)
        .first(&mut pg_conn)
        .optional()
    {
        Ok(res) => res,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting engineer name from postgres");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if name.is_none() {
        return (StatusCode::BAD_REQUEST, "provided engineer was not found").into_response();
    }

    Json(GetUserNameResponse {
        name: name.unwrap(),
    })
    .into_response()
}

#[utoipa::path(
    post,
    path="/api/auth/login",
    description="Login into existing user profile and get refresh and access tokens",
    request_body=LoginRequest,
    responses(
        (status=200, description="Successful login", body=LoginResponse),
        (status=401, description="Login failed because of credentials", body=String),
        (status=500, description="Server-side error happened", body=String)
    )
)]
pub async fn login_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> Response<Body> {
    if let Err(e) = payload.validate() {
        return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
    }

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

    let found_engineer = match schema::engineers::table
        .filter(schema::engineers::email.eq(&payload.email))
        .select(Engineer::as_select())
        .first(&mut pg_conn)
        .optional()
    {
        Ok(opt) => opt,
        Err(e) => {
            tracing::error!(error = %e, "db error while looking up engineer");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    let found_dispatcher = match schema::dispatchers::table
        .filter(schema::dispatchers::email.eq(&payload.email))
        .select(Dispatcher::as_select())
        .first(&mut pg_conn)
        .optional()
    {
        Ok(opt) => opt,
        Err(e) => {
            tracing::error!(error = %e, "db error while looking up dispatcher");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    let (user, role) = match (found_engineer, found_dispatcher) {
        (None, None) => {
            return (StatusCode::UNAUTHORIZED, "incorrect login or password").into_response();
        }
        (None, Some(d)) => (AuthenticatedUser::Dispatcher(d), UserRole::Dispatcher),
        (Some(e), None) => (AuthenticatedUser::Engineer(e), UserRole::Engineer),
        (Some(e), Some(_)) => {
            tracing::error!(email = %e.email, "found duplicate in both tables: dispatchers and engineers");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    match verify_password(payload.password.as_str(), user.password_hash()) {
        Ok(_) => (),
        Err(_) => {
            return (StatusCode::UNAUTHORIZED, "incorrect login or password").into_response();
        }
    }

    let (access, refresh) = match create_token_pair(
        user.id(),
        user.role(),
        app_state.jwt_secret.as_bytes(),
    ) {
        Ok((a, r)) => (a, r),
        Err(e) => {
            tracing::error!(uuid = %&user.id(), error = %e, "error during creating token pair for user");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    if let Err(e) = revoke_and_store_tokens(&mut redis_conn, user.id(), &access, &refresh, &role) {
        tracing::error!(error = %e, "error during storing tokens in redis");
        return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
    }

    Json(LoginResponse {
        name: user.name().to_owned(),
        access_token: access,
        refresh_token: refresh,
        user_role: role,
    })
    .into_response()
}

#[utoipa::path(
    post,
    path="/api/auth/update_access",
    description="Update access token using refresh token",
    request_body=UpdateAccessRequest,
    responses(
        (status=200, description="Successful update", body=UpdateAccessResponse),
        (status=401, description="Refresh token is no longer valid. You have to relogin", body=String),
        (status=500, description="Server-side error happened", body=String)
    )
)]
pub async fn update_access_token(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<UpdateAccessRequest>,
) -> Response<Body> {
    let mut redis_conn = match app_state.redis_pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Error during extracting redis connection from pool");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let user_id = match redis_conn.get(format!("refresh:{}", payload.refresh_token)) {
        Ok(Some(id)) => match Uuid::from_str(&id) {
            Ok(uuid) => uuid,
            Err(e) => {
                tracing::error!(error = %e, "corrupted user_id stored for refresh token");
                return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
            }
        },
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                "refresh token is not exists or was expired",
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "error during getting user id from redis by refresh token");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    let role = match redis_conn.get(format!("role:{}", user_id)) {
        Ok(Some(found)) => found,
        Ok(None) => {
            tracing::error!(user_id = %user_id, "user found via refresh, but role missing");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to get user role from redis");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    let role = match UserRole::try_from(role) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "error during parsing user role from redis, architecture critical issue");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    let access = match create_token_pair(user_id, role, app_state.jwt_secret.as_bytes()) {
        Ok((a, _)) => a,
        Err(e) => {
            tracing::error!(uuid = %user_id, error = %e, "error during creating access token");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    if let Err(e) = revoke_and_store_access_token(&mut redis_conn, user_id, &access) {
        tracing::error!(error = %e, "error during storing new access token");
        return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
    }

    Json(UpdateAccessResponse {
        access_token: access,
    })
    .into_response()
}
