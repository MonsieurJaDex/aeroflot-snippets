//! Authentication handlers: registration, login, token refresh, name lookup.

use std::sync::Arc;

use axum::{
    Extension, Json,
    body::Body,
    extract::{Path, State},
    http::Response,
    response::IntoResponse,
};
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl, SelectableHelper};
use redis::TypedCommands;
use uuid::Uuid;
use validator::Validate;

use super::{bad_request, internal_error, not_found, unauthorized};
use crate::{
    database::schema,
    middleware::AuthUser,
    models::{dispatcher::Dispatcher, engineer::Engineer},
    types::{
        config::AppState,
        dto::{
            GetUserNameResponse, LoginRequest, LoginResponse, MeResponse, RegisterRequest,
            UpdateAccessRequest, UpdateAccessResponse,
        },
        enums::{AuthenticatedUser, UserRole},
    },
    utils::{
        create_token_pair, hash_password, revoke_and_store_access_token, revoke_and_store_tokens,
        verify_password,
    },
};

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Registers a new user (dispatcher or engineer) and issues a token pair.
#[utoipa::path(
    post,
    path = "/api/auth/register",
    description = "Register a new user profile and obtain access + refresh tokens",
    request_body = RegisterRequest,
    responses(
        (status = 200, description = "Successful registration", body = LoginResponse),
        (status = 400, description = "Validation failure or duplicate email", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
)]
pub async fn register_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Response<Body>, Response<Body>> {
    payload.validate().map_err(|e| bad_request(e.to_string()))?;

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    if email_taken(&mut pg_conn, &payload.email)? {
        return Err(bad_request("email already registered"));
    }

    let hashed_password = hash_password(payload.password.clone()).map_err(|e| {
        tracing::error!(error = %e, "failed to hash password");
        internal_error()
    })?;

    let new_id = Uuid::new_v4();

    match payload.user_role {
        UserRole::Dispatcher => {
            let new_dispatcher = Dispatcher {
                id: new_id,
                name: payload.name.clone(),
                email: payload.email.clone(),
                password_hash: hashed_password,
            };
            insert_dispatcher(&mut pg_conn, &new_dispatcher)?;
        }
        UserRole::Engineer => {
            let engineer_type = payload.engineer_type.ok_or_else(|| {
                bad_request("user_role = engineer requires engineer_type to be set")
            })?;

            let new_engineer = Engineer {
                id: new_id,
                email: payload.email.clone(),
                name: payload.name.clone(),
                password_hash: hashed_password,
                engineer_type,
            };
            insert_engineer(&mut pg_conn, &new_engineer)?;
        }
    }

    let (access, refresh) = create_token_pair(
        new_id,
        payload.user_role.clone(),
        app_state.jwt_secret.as_bytes(),
    )
    .map_err(|e| {
        tracing::error!(uuid = %new_id, error = %e, "failed to create token pair");
        internal_error()
    })?;

    revoke_and_store_tokens(
        &mut redis_conn,
        new_id,
        &access,
        &refresh,
        &payload.user_role,
    )
    .map_err(|e| {
        tracing::error!(error = %e, "failed to store tokens in redis");
        internal_error()
    })?;

    Ok(Json(LoginResponse {
        id: new_id.to_string(),
        name: payload.name,
        user_role: payload.user_role,
        access_token: access,
        refresh_token: refresh,
    })
    .into_response())
}

/// True when the address is already stored for a dispatcher or an engineer.
fn email_taken(conn: &mut diesel::PgConnection, email: &str) -> Result<bool, Response<Body>> {
    let engineer_exists = diesel::select(diesel::dsl::exists(
        schema::engineers::table.filter(schema::engineers::email.eq(email)),
    ))
    .get_result(conn)
    .map_err(|e| {
        tracing::error!(error = %e, "failed to check engineer email");
        internal_error()
    })?;
    if engineer_exists {
        return Ok(true);
    }

    diesel::select(diesel::dsl::exists(
        schema::dispatchers::table.filter(schema::dispatchers::email.eq(email)),
    ))
    .get_result(conn)
    .map_err(|e| {
        tracing::error!(error = %e, "failed to check dispatcher email");
        internal_error()
    })
}

fn insert_dispatcher(
    conn: &mut diesel::PgConnection,
    dispatcher: &Dispatcher,
) -> Result<(), Response<Body>> {
    diesel::insert_into(schema::dispatchers::table)
        .values(dispatcher)
        .on_conflict_do_nothing()
        .execute(conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to insert dispatcher");
            internal_error()
        })
        .and_then(|inserted| {
            if inserted == 0 {
                Err(bad_request("dispatcher with this email already exists"))
            } else {
                Ok(())
            }
        })
}

fn insert_engineer(
    conn: &mut diesel::PgConnection,
    engineer: &Engineer,
) -> Result<(), Response<Body>> {
    diesel::insert_into(schema::engineers::table)
        .values(engineer)
        .on_conflict_do_nothing()
        .execute(conn)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to insert engineer");
            internal_error()
        })
        .and_then(|inserted| {
            if inserted == 0 {
                Err(bad_request("engineer with this email already exists"))
            } else {
                Ok(())
            }
        })
}

// ---------------------------------------------------------------------------
// Login
// ---------------------------------------------------------------------------

/// Authenticates an existing user and returns a fresh JWT pair.
#[utoipa::path(
    post,
    path = "/api/auth/login",
    description = "Login and obtain access + refresh tokens",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Successful login", body = LoginResponse),
        (status = 400, description = "Validation failure", body = String),
        (status = 401, description = "Invalid credentials", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
)]
pub async fn login_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> Result<Response<Body>, Response<Body>> {
    payload.validate().map_err(|e| bad_request(e.to_string()))?;

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    // Look up the email in both tables; exactly one hit expected.
    let engineer = schema::engineers::table
        .filter(schema::engineers::email.eq(&payload.email))
        .select(Engineer::as_select())
        .first(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "db error looking up engineer");
            internal_error()
        })?;

    let dispatcher = schema::dispatchers::table
        .filter(schema::dispatchers::email.eq(&payload.email))
        .select(Dispatcher::as_select())
        .first(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "db error looking up dispatcher");
            internal_error()
        })?;

    let (user, role) = match (engineer, dispatcher) {
        (None, None) => return Err(unauthorized("invalid email or password")),
        (Some(_), Some(_)) => {
            tracing::error!(email = %payload.email, "email duplicated across both tables");
            return Err(internal_error());
        }
        (Some(e), None) => (AuthenticatedUser::Engineer(e), UserRole::Engineer),
        (None, Some(d)) => (AuthenticatedUser::Dispatcher(d), UserRole::Dispatcher),
    };

    verify_password(payload.password.as_str(), user.password_hash())
        .map_err(|_| unauthorized("invalid email or password"))?;

    let (access, refresh) =
        create_token_pair(user.id(), user.role(), app_state.jwt_secret.as_bytes()).map_err(
            |e| {
                tracing::error!(uuid = %user.id(), error = %e, "failed to create token pair");
                internal_error()
            },
        )?;

    revoke_and_store_tokens(&mut redis_conn, user.id(), &access, &refresh, &role).map_err(|e| {
        tracing::error!(error = %e, "failed to store tokens in redis");
        internal_error()
    })?;

    Ok(Json(LoginResponse {
        id: user.id().to_string(),
        name: user.name().to_owned(),
        access_token: access,
        refresh_token: refresh,
        user_role: role,
    })
    .into_response())
}

// ---------------------------------------------------------------------------
// Name lookup
// ---------------------------------------------------------------------------

/// Returns the display name of an engineer by UUID.
#[utoipa::path(
    get,
    path = "/api/auth/getEngineerName/{engineer_uuid}",
    description = "Fetch the display name of an engineer by UUID",
    responses(
        (status = 200, description = "Engineer found", body = GetUserNameResponse),
        (status = 400, description = "Malformed UUID", body = String),
        (status = 404, description = "Engineer not found", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
)]
pub async fn get_engineer_name_handler(
    State(app_state): State<Arc<AppState>>,
    Path(engineer_uuid): Path<String>,
) -> Result<Response<Body>, Response<Body>> {
    let uuid = Uuid::parse_str(&engineer_uuid).map_err(|_| bad_request("invalid UUID format"))?;

    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;

    let name = schema::engineers::table
        .filter(schema::engineers::id.eq(uuid))
        .select(schema::engineers::name)
        .first::<String>(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to fetch engineer name");
            internal_error()
        })?
        .ok_or_else(|| not_found("engineer not found"))?;

    Ok(Json(GetUserNameResponse { name }).into_response())
}

/// Returns the authenticated user's profile. The password hash is never included.
#[utoipa::path(
    get,
    path = "/api/auth/me",
    responses(
        (status = 200, description = "Current user profile", body = MeResponse),
        (status = 401, description = "Missing or invalid access token", body = String),
        (status = 404, description = "User no longer exists", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
    params(
        ("Authorization" = String, Header, description = "Bearer access token"),
    ),
)]
pub async fn me_handler(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response<Body>, Response<Body>> {
    let mut pg_conn = app_state.db_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire postgres connection");
        internal_error()
    })?;

    if let Some(engineer) = schema::engineers::table
        .filter(schema::engineers::id.eq(auth_user.id))
        .select(Engineer::as_select())
        .first(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load engineer profile");
            internal_error()
        })?
    {
        return Ok(Json(MeResponse {
            id: engineer.id.to_string(),
            email: engineer.email,
            name: engineer.name,
            user_role: UserRole::Engineer,
            engineer_type: Some(engineer.engineer_type),
        })
        .into_response());
    }

    let dispatcher = schema::dispatchers::table
        .filter(schema::dispatchers::id.eq(auth_user.id))
        .select(Dispatcher::as_select())
        .first(&mut pg_conn)
        .optional()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to load dispatcher profile");
            internal_error()
        })?
        .ok_or_else(|| not_found("user not found"))?;

    Ok(Json(MeResponse {
        id: dispatcher.id.to_string(),
        email: dispatcher.email,
        name: dispatcher.name,
        user_role: UserRole::Dispatcher,
        engineer_type: None,
    })
    .into_response())
}

// ---------------------------------------------------------------------------
// Token refresh
// ---------------------------------------------------------------------------

/// Issues a new access token given a valid refresh token.
#[utoipa::path(
    post,
    path = "/api/auth/update_access",
    description = "Obtain a new access token using a valid refresh token",
    request_body = UpdateAccessRequest,
    responses(
        (status = 200, description = "New access token", body = UpdateAccessResponse),
        (status = 401, description = "Refresh token is invalid or expired", body = String),
        (status = 500, description = "Server-side error", body = String),
    ),
)]
pub async fn update_access_token(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<UpdateAccessRequest>,
) -> Result<Response<Body>, Response<Body>> {
    let mut redis_conn = app_state.redis_pool.get().map_err(|e| {
        tracing::error!(error = %e, "failed to acquire redis connection");
        internal_error()
    })?;

    let user_id = redis_conn
        .get(format!("refresh:{}", payload.refresh_token))
        .map_err(|e| {
            tracing::error!(error = %e, "failed to fetch user id by refresh token");
            internal_error()
        })?
        .ok_or_else(|| unauthorized("refresh token is invalid or expired"))?;

    let user_id = Uuid::from_str(&user_id).map_err(|e| {
        tracing::error!(error = %e, "corrupted user id stored for refresh token");
        internal_error()
    })?;

    let role_str = redis_conn
        .get(format!("role:{}", user_id))
        .map_err(|e| {
            tracing::error!(error = %e, "failed to fetch user role from redis");
            internal_error()
        })?
        .ok_or_else(|| {
            tracing::error!(%user_id, "user has refresh token but no stored role");
            internal_error()
        })?;

    let role = UserRole::try_from(role_str).map_err(|e| {
        tracing::error!(error = %e, "stored user role could not be parsed");
        internal_error()
    })?;

    // NOTE: `create_token_pair` also generates a refresh token we throw away.
    // A dedicated `create_access_token` helper would avoid the extra work.
    let (access, _) =
        create_token_pair(user_id, role, app_state.jwt_secret.as_bytes()).map_err(|e| {
            tracing::error!(uuid = %user_id, error = %e, "failed to create access token");
            internal_error()
        })?;

    revoke_and_store_access_token(&mut redis_conn, user_id, &access).map_err(|e| {
        tracing::error!(error = %e, "failed to store new access token");
        internal_error()
    })?;

    Ok(Json(UpdateAccessResponse {
        access_token: access,
    })
    .into_response())
}

use std::str::FromStr;
