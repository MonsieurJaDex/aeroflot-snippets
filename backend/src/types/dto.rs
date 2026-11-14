//! Request and response payloads for HTTP handlers.
//!
//! All structures here are annotated with [`utoipa::ToSchema`] so they are
//! exported to the generated OpenAPI specification. Structures that hold
//! sensitive material (passwords, tokens) provide a **redacted** [`Debug`]
//! implementation to avoid accidental leakage in logs.

use std::fmt;

use crate::types::{
    enums::{AircraftIssue, EngineerType, UserRole, VehicleType},
    map::{Point, Route},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

// ---------------------------------------------------------------------------
// Route handling
// ---------------------------------------------------------------------------

/// Response payload for the `get_route` endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct GetRouteResponse {
    /// Ordered sequence of points that form the shortest path.
    pub route: Route,
    /// Total path length in map cells.
    pub distance: usize,
}

/// Request payload for the `get_route` endpoint.
#[derive(Debug, Deserialize, ToSchema)]
pub struct GetRouteRequest {
    /// Starting point of the route.
    pub start_point: Point,
    /// Destination point of the route.
    pub end_point: Point,
}

// ---------------------------------------------------------------------------
// Engineer assignment
// ---------------------------------------------------------------------------

/// Request payload for the `assign_engineer` endpoint.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssignEngineerRequest {
    /// Type of the reported aircraft issue.
    pub issue: AircraftIssue,
    /// UUID of the aircraft stored in Redis (`position:aircraft:{uuid}`).
    #[schema(example = "3fa85f64-5717-4562-b3fc-2c963f66afa6")]
    pub plane_uuid: String,
    /// Free-form problem description supplied by the dispatcher.
    pub description: String,
}

/// One aircraft currently stored in Redis.
#[derive(Debug, Serialize, ToSchema)]
pub struct AircraftEntry {
    /// Aircraft UUID.
    pub id: uuid::Uuid,
    /// Current position on the map.
    pub point: Point,
}

/// Aircraft positions visible to a dispatcher.
#[derive(Debug, Serialize, ToSchema)]
pub struct AircraftListResponse {
    pub aircraft: Vec<AircraftEntry>,
}

/// Response payload for the `assign_engineer` endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssignEngineerResponse {
    /// UUID of the engineer chosen for this task.
    pub engineer_uuid: String,
    /// Estimated resolution time in seconds.
    pub time: u64,
    /// Whether the assignment exceeds the allowed SLA.
    pub time_limit_exceeded: bool,
    /// Route the engineer must follow to reach the aircraft.
    pub route: Route,
}

/// Response payload describing the engineer's current task.
#[derive(Debug, Serialize, ToSchema)]
pub struct CurrentTaskResponse {
    /// Unique task identifier.
    pub id: String,
    /// Human-readable task description.
    pub description: String,
    /// Reported aircraft issue.
    pub issue: AircraftIssue,
    /// Location of the aircraft on the map at the time of assignment.
    pub plane_point: Point,
    /// UUID of the aircraft attached to this task.
    pub plane_uuid: Option<String>,
    /// Route from the engineer to the aircraft.
    pub route: Route,
}

// ---------------------------------------------------------------------------
// User lookup
// ---------------------------------------------------------------------------

/// Request payload for the `get_engineer_name` endpoint.

/// Profile of the authenticated user. Never includes the password hash.
#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    pub id: String,
    pub email: String,
    pub name: String,
    pub user_role: UserRole,
    /// Set only when the caller is an engineer.
    pub engineer_type: Option<EngineerType>,
}

/// Response payload for the `get_engineer_name` endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct GetUserNameResponse {
    /// Display name of the engineer.
    #[schema(example = "Ivan Ivanov")]
    pub name: String,
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

/// Request payload for the `register` endpoint.
#[derive(Deserialize, ToSchema, Validate)]
pub struct RegisterRequest {
    /// User's email address (validated as an RFC 5322 email).
    #[validate(email)]
    #[schema(example = "engineer@aeroflot.example")]
    pub email: String,

    /// Display name shown in the UI.
    #[schema(example = "Ivan Ivanov")]
    pub name: String,

    /// Plaintext password. **Never logged or stored in clear text.**
    #[schema(write_only)]
    pub password: String,

    /// Role to be assigned to the new account.
    pub user_role: UserRole,

    /// Required when `user_role == engineer`, ignored otherwise.
    pub engineer_type: Option<EngineerType>,
}

impl fmt::Debug for RegisterRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisterRequest")
            .field("email", &self.email)
            .field("name", &self.name)
            .field("password", &"<redacted>")
            .field("user_role", &self.user_role)
            .field("engineer_type", &self.engineer_type)
            .finish()
    }
}

/// Request payload for the `login` endpoint.
#[derive(Deserialize, ToSchema, Validate)]
pub struct LoginRequest {
    /// User's email address.
    #[validate(email)]
    #[schema(example = "engineer@aeroflot.example")]
    pub email: String,

    /// Plaintext password.
    #[schema(write_only)]
    pub password: String,
}

impl fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginRequest")
            .field("email", &self.email)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Response payload for the `login` endpoint.
#[derive(Serialize, ToSchema)]
pub struct LoginResponse {
    /// User UUID
    pub id: String,
    /// Display name of the authenticated user.
    pub name: String,
    /// Role of the authenticated user.
    pub user_role: UserRole,
    /// Short-lived access token.
    pub access_token: String,
    /// Long-lived refresh token.
    pub refresh_token: String,
}

impl fmt::Debug for LoginResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginResponse")
            .field("name", &self.name)
            .field("user_role", &self.user_role)
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .finish()
    }
}

/// Request payload for the `update_access_token` endpoint.
#[derive(Deserialize, ToSchema)]
pub struct UpdateAccessRequest {
    /// Valid refresh token.
    #[schema(write_only)]
    pub refresh_token: String,
}

impl fmt::Debug for UpdateAccessRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateAccessRequest")
            .field("refresh_token", &"<redacted>")
            .finish()
    }
}

/// Response payload for the `update_access_token` endpoint.
#[derive(Serialize, ToSchema)]
pub struct UpdateAccessResponse {
    /// Fresh access token.
    pub access_token: String,
}

impl fmt::Debug for UpdateAccessResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateAccessResponse")
            .field("access_token", &"<redacted>")
            .finish()
    }
}

/// Request payload for setting or clearing AOG on the engineer's current task.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CurrentTaskAogRequest {
    /// `true` marks the task's aircraft as AOG, `false` clears the status.
    pub toggle: bool,
}

/// Response payload for the current-task AOG endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct CurrentTaskAogResponse {
    /// UUID of the aircraft whose status was updated.
    pub plane_uuid: String,
    /// Whether the aircraft is currently marked AOG.
    pub aog: bool,
}

/// Aircraft that currently have an AOG status in Redis.
#[derive(Debug, Serialize, ToSchema)]
pub struct AogListResponse {
    /// UUIDs of aircraft marked AOG.
    pub plane_uuids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Geo
// ---------------------------------------------------------------------------

/// One buffered engineer fix: `[point, unix_timestamp]`.
///
/// The timestamp is seconds since the Unix epoch, recorded when the device
/// produced the point.
#[derive(Debug, Deserialize, ToSchema)]
pub struct EngineerPositionSample(pub Point, pub i64);

/// Request payload for flushing a buffered batch of engineer positions.
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateEngineerPositionRequest {
    /// UUID of the engineer.
    pub id: String,
    /// Fixes the device buffered before it could send them.
    ///
    /// Each item is `[point, unix_timestamp]`. The handler keeps the fix with
    /// the greatest timestamp.
    pub batch: Vec<EngineerPositionSample>,
}

/// Request payload for updating a special vehicle's position in the simulator.
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateTransportPositionRequest {
    /// UUID of the special vehicle.
    pub id: String,
    /// Type of the special vehicle.
    pub vehicle_type: VehicleType,
    /// New position on the map.
    pub new_point: Point,
}
