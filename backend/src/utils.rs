use anyhow::anyhow;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::{Duration, Utc};
use jsonwebtoken::{EncodingKey, Header};
use uuid::Uuid;

pub const ACCESS_TTL_SECS: i64 = 3600;
pub const REFRESH_TTL_SECS: i64 = 604800;

use crate::types::{
    auth::{AccessClaims, RefreshClaims},
    enums::{TokenType, UserRole},
};

use redis::{Connection, TypedCommands};

pub fn revoke_and_store_tokens(
    redis_conn: &mut Connection,
    user_id: Uuid,
    access: &str,
    refresh: &str,
    role: &UserRole,
) -> Result<(), redis::RedisError> {
    let old_access: Option<String> = redis_conn.get(format!("access_from_id:{}", user_id))?;
    let old_refresh: Option<String> = redis_conn.get(format!("refresh_from_id:{}", user_id))?;

    let mut pipe = redis::pipe();
    pipe.atomic();
    if let Some(old) = old_access.as_deref() {
        pipe.del(format!("access:{}", old)).ignore();
    }
    if let Some(old) = old_refresh.as_deref() {
        pipe.del(format!("refresh:{}", old)).ignore();
    }
    pipe.set_ex(format!("refresh:{}", refresh), user_id.to_string(), REFRESH_TTL_SECS as u64).ignore();
    pipe.set_ex(format!("refresh_from_id:{}", user_id), refresh, REFRESH_TTL_SECS as u64).ignore();
    pipe.set_ex(format!("access:{}", access), user_id.to_string(), ACCESS_TTL_SECS as u64).ignore();
    pipe.set_ex(format!("access_from_id:{}", user_id), access, ACCESS_TTL_SECS as u64).ignore();
    pipe.set_ex(format!("role:{}", user_id), role.to_string(), REFRESH_TTL_SECS as u64).ignore();
    pipe.query::<()>(redis_conn)?;
    Ok(())
}

pub fn revoke_and_store_access_token(
    redis_conn: &mut Connection,
    user_id: Uuid,
    access: &str,
) -> Result<(), redis::RedisError> {
    let old_access: Option<String> = redis_conn.get(format!("access_from_id:{}", user_id))?;

    let mut pipe = redis::pipe();
    pipe.atomic();
    if let Some(old) = old_access.as_deref() {
        pipe.del(format!("access:{}", old)).ignore();
    }
    pipe.set_ex(format!("access:{}", access), user_id.to_string(), ACCESS_TTL_SECS as u64).ignore();
    pipe.set_ex(format!("access_from_id:{}", user_id), access, ACCESS_TTL_SECS as u64).ignore();
    pipe.query::<()>(redis_conn)?;
    Ok(())
}

pub fn hash_password(password: String) -> anyhow::Result<String> {
    let argon2 = Argon2::default();
    Ok(argon2.hash_password(password.as_bytes())?.to_string())
}

pub fn verify_password(provided: &str, password_hash: &str) -> anyhow::Result<()> {
    let argon2 = Argon2::default();
    let parsed = PasswordHash::new(password_hash)?;
    argon2
        .verify_password(provided.as_bytes(), &parsed)
        .map_err(|e| anyhow!(e))
}

pub fn create_token_pair(
    user_id: Uuid,
    role: UserRole,
    secret: &[u8],
) -> anyhow::Result<(String, String)> {
    let access_exp = (Utc::now() + Duration::seconds(ACCESS_TTL_SECS)).timestamp() as usize;
    let access_claims = AccessClaims {
        sub: user_id,
        role,
        exp: access_exp,
        token_type: TokenType::Access,
    };

    let access_token = jsonwebtoken::encode(
        &Header::default(),
        &access_claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )?;

    let refresh_jti = Uuid::new_v4();
    let refresh_exp = (Utc::now() + Duration::seconds(REFRESH_TTL_SECS)).timestamp() as usize;
    let refresh_claims = RefreshClaims {
        sub: user_id,
        exp: refresh_exp,
        jti: refresh_jti,
        token_type: TokenType::Refresh,
    };
    let refresh_token = jsonwebtoken::encode(
        &Header::default(),
        &refresh_claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )?;

    Ok((access_token, refresh_token))
}
