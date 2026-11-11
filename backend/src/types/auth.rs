//! JWT claim types used for signing and verifying authentication tokens.
//!
//! Two kinds of tokens are issued:
//! * **Access** tokens — short-lived, carry the user's [`UserRole`] and are
//!   accepted by protected endpoints.
//! * **Refresh** tokens — long-lived, carry a unique [`jti`](RefreshClaims::jti)
//!   that may be used for revocation / rotation.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::enums::{TokenType, UserRole};

/// Claims embedded in a signed JWT **access** token.
///
/// The authentication middleware verifies that [`Self::token_type`] equals
/// [`TokenType::Access`] and that [`Self::exp`] is in the future before
/// forwarding the request to the handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessClaims {
    /// Subject — unique identifier of the authenticated user.
    pub sub: Uuid,
    /// Role of the authenticated user (`dispatcher` or `engineer`).
    pub role: UserRole,
    /// Expiration timestamp (UNIX epoch, seconds).
    pub exp: usize,
    /// Distinguishes access tokens from refresh tokens.
    pub token_type: TokenType,
}

/// Claims embedded in a signed JWT **refresh** token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshClaims {
    /// Subject — unique identifier of the authenticated user.
    pub sub: Uuid,
    /// Expiration timestamp (UNIX epoch, seconds).
    pub exp: usize,
    /// Unique token id used for revocation / rotation.
    pub jti: Uuid,
    /// Distinguishes refresh tokens from access tokens.
    pub token_type: TokenType,
}

impl AccessClaims {
    /// Returns `true` if the token has expired at the given UNIX timestamp.
    #[must_use]
    pub fn is_expired(&self, now_secs: usize) -> bool {
        self.exp <= now_secs
    }

    /// Returns `true` if this token is indeed an access token.
    #[must_use]
    pub fn is_access(&self) -> bool {
        matches!(self.token_type, TokenType::Access)
    }
}

impl RefreshClaims {
    /// Returns `true` if the token has expired at the given UNIX timestamp.
    #[must_use]
    pub fn is_expired(&self, now_secs: usize) -> bool {
        self.exp <= now_secs
    }

    /// Returns `true` if this token is indeed a refresh token.
    #[must_use]
    pub fn is_refresh(&self) -> bool {
        matches!(self.token_type, TokenType::Refresh)
    }
}