//! End-to-end API tests. They run against a real PostgreSQL database, a
//! dedicated Redis database and the real Axum router. See [`support`] for the
//! connection defaults and reset rules.

mod aircraft;
mod assign;
mod auth;
mod geo;
mod map_v2;
mod maps;
mod requests;
mod support;
