use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::enums::{TokenType, UserRole};

#[derive(Serialize, Deserialize)]
pub struct AccessClaims {
    pub sub: Uuid,
    pub role: UserRole,
    pub exp: usize,
    pub token_type: TokenType,
}

#[derive(Serialize, Deserialize)]
pub struct RefreshClaims {
    pub sub: Uuid,
    pub exp: usize,
    pub jti: Uuid,
    pub token_type: TokenType,
}
