//! Common domain types shared across the backend.
//!
//! The module groups together several logically distinct sub-modules:
//!
//! | Module   | Purpose                                                              |
//! |----------|----------------------------------------------------------------------|
//! | [`auth`] | JWT claims for access / refresh tokens.                              |
//! | [`config`] | Runtime configuration and shared application state.                 |
//! | [`doc`]  | OpenAPI (utoipa) documentation aggregator.                           |
//! | [`dto`]  | Request / response payloads exposed by HTTP handlers.                |
//! | [`enums`] | Domain enumerations (engineer types, aircraft issues, roles, …).    |
//! | [`map`]  | Airport map primitives ([`MapMatrix`], [`Point`], [`Route`]).        |

pub mod auth;
pub mod config;
pub mod doc;
pub mod dto;
pub mod enums;
pub mod map;