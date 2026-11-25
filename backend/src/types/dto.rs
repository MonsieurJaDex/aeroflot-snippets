//! Request and response payloads for HTTP handlers.
//!
//! All structures here are annotated with [`utoipa::ToSchema`] so they are
//! exported to the generated OpenAPI specification. Structures that hold
//! sensitive material (passwords, tokens) provide a **redacted** [`Debug`]
//! implementation to avoid accidental leakage in logs.

use std::{collections::BTreeMap, fmt};

use crate::types::{
    enums::{AircraftIssue, EngineerType, ObjectKind, UserRole, VehicleType},
    map::{Point, Route},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

// ---------------------------------------------------------------------------
// Route handling
// ---------------------------------------------------------------------------

/// Current airport map and the time it was loaded.
#[derive(Debug, Serialize, ToSchema)]
#[schema(description = "Airport map served to authenticated clients.")]
pub struct MapResponse {
    /// File stem of the map currently being served.
    pub id: String,
    /// Display name, equal to the file stem.
    pub name: String,
    /// Grid of walkable cells and obstacles. Values are palette ids, `0` is
    /// empty, so a client must read `palette` to interpret them.
    pub matrix: crate::types::map::MapMatrix,
    /// Map format generation: `2` for a palette matrix.
    pub format: u32,
    /// Width in cells.
    pub width: usize,
    /// Height in cells.
    pub height: usize,
    /// Side of one cell in metres.
    pub cell_size_m: f64,
    /// North-west corner of cell `(0, 0)`.
    pub origin: crate::types::map::Origin,
    /// Every class a matrix cell may carry.
    pub palette: Vec<crate::types::map::PaletteEntry>,
    /// Which palette ids are drivable and which are walkable.
    pub layers: crate::types::map::MapLayers,
    /// Buildings and other named areas.
    pub objects: Vec<crate::types::map::MapObject>,
    /// Service zones.
    pub zones: Vec<crate::types::map::MapZone>,
    /// Cells claimed by more than one class. `matrix` holds only the winner, so
    /// a client that needs both speeds on an overlapped cell reads this.
    #[serde(default)]
    pub overlays: Vec<crate::types::map::CellOverlay>,
    /// When this process loaded the map file.
    #[schema(format = "date-time")]
    pub updated_at: String,
}

/// One map the server can switch to.
#[derive(Debug, Serialize, ToSchema)]
#[schema(description = "A map file discovered next to the CLI map.")]
pub struct MapSummary {
    /// File stem of the map, used as the switchable id.
    pub id: String,
    /// Display name, equal to the file stem.
    pub name: String,
    /// Map width in cells.
    pub width: i64,
    /// Map height in cells.
    pub height: i64,
    /// True for the map routing and Redis positions currently use.
    pub active: bool,
}

/// Catalog of maps and the id that is active.
#[derive(Debug, Serialize, ToSchema)]
pub struct MapListResponse {
    /// All map files the server can switch to.
    pub maps: Vec<MapSummary>,
    /// Id of the map currently active.
    pub active_id: String,
}

/// Active map without the matrix.
#[derive(Debug, Serialize, ToSchema)]
pub struct ActiveMapResponse {
    /// File stem of the active map.
    pub id: String,
    /// Display name, equal to the file stem.
    pub name: String,
    /// Map width in cells.
    pub width: i64,
    /// Map height in cells.
    pub height: i64,
    /// When the server loaded the map file.
    #[schema(format = "date-time")]
    pub updated_at: String,
}

/// Body of `PUT /api/maps/active`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectMapRequest {
    /// Id from `GET /api/maps`.
    pub id: String,
}

/// Response payload for the `get_route` endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct GetRouteResponse {
    /// Ordered sequence of points that form the shortest path.
    pub route: Route,
    /// Total path length in map cells.
    pub distance: usize,
    /// Estimated travel time in seconds at the per-section speeds of the map.
    /// Two routes of equal `distance` differ here when they cross different
    /// speed classes (a 20 km/h service section against a 5 km/h one).
    pub eta_seconds: u64,
    /// Who the route was planned for.
    pub agent: RouteAgent,
}

/// Query window for collection endpoints.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// Maximum number of items to return.
    #[param(minimum = 1, maximum = 1000, default = 100)]
    pub limit: Option<i64>,
    /// Number of items to skip.
    #[param(minimum = 0, default = 0)]
    pub offset: Option<i64>,
}

/// Body of `POST /api/geo/pauseAgent`.
///
/// The organiser checklist asks for a way to stop an agent without ending its
/// route, so the test can reproduce a halted engineer.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PauseAgentRequest {
    /// Object a tracker reports.
    pub obj: String,
    /// `true` stops the agent where it stands, `false` lets it move again.
    pub paused: bool,
}

/// Answer of `POST /api/geo/pauseAgent`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PauseAgentResponse {
    pub obj: String,
    pub paused: bool,
}

/// Body of `POST /api/geo/forceVehicle`.
///
/// The organiser checklist asks for a way to make an engineer use a vehicle even
/// when walking would be faster, so the vehicle branch of a candidate can be
/// demonstrated.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ForceVehicleRequest {
    pub engineer_id: Uuid,
    /// `true` always sends the engineer by vehicle, `false` restores the normal
    /// comparison with walking.
    pub forced: bool,
}

/// Answer of `POST /api/geo/forceVehicle`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ForceVehicleResponse {
    pub engineer_id: Uuid,
    pub forced: bool,
}

/// Query of `GET /api/geo/trackFreshness`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TrackFreshnessQuery {
    /// Object a tracker reports; the same `obj` the points were sent with.
    pub obj: String,
}

/// Freshness of an object's coordinate, as `GET /api/geo/trackFreshness` answers
/// it. The key is present only while the tracker keeps reporting.
#[derive(Debug, Serialize, ToSchema)]
pub struct TrackFreshnessResponse {
    /// Object that was asked about, echoed back.
    pub obj: String,
    /// `true` when Redis holds a live coordinate for the object, `false` when the
    /// last point is older than `stale_after_secs` or was never received.
    pub fresh: bool,
    /// Seconds left before the coordinate expires; `0` when it already has.
    #[schema(minimum = 0)]
    pub expires_in_secs: i64,
}

/// Uniform JSON error body.
#[derive(Debug, Serialize, ToSchema)]
#[schema(description = "Error returned by every non-2xx response.")]
pub struct ErrorResponse {
    /// Stable machine-readable code, for example `VALIDATION_ERROR`.
    #[schema(example = "VALIDATION_ERROR")]
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
    /// Optional structured context. Keys are not fixed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Map<String, serde_json::Value>>,
    /// Request id assigned by the server, when the caller sent or received one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Who travels the route. A pedestrian ignores the posted road speeds (a
/// person walks at the same pace on an apron and on an airfield), while a
/// ground vehicle keeps the speed of each section and therefore pays very
/// differently on a 5 km/h service road and on a 20 km/h one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RouteAgent {
    /// An engineer on foot (default): walkable cells only, `Walk` speed.
    #[default]
    Engineer,
    /// A ground vehicle: drivable cells only, the posted speed of each section.
    Vehicle,
}

/// Request payload for the `get_route` endpoint.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GetRouteRequest {
    /// Starting point of the route.
    pub start_point: Point,
    /// Destination point of the route.
    pub end_point: Point,
    /// Who travels. Omitted means `engineer`, keeping the old behaviour.
    #[serde(default)]
    pub agent: RouteAgent,
}

// ---------------------------------------------------------------------------
// Engineer assignment
// ---------------------------------------------------------------------------

/// Request payload for the `assign_engineer` endpoint.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignEngineerRequest {
    /// Known issue category. Defaults to `other` when omitted.
    #[serde(default)]
    #[allow(dead_code)]
    pub issue: AircraftIssue,
    /// Free-form text passed to the Bayes classifier.
    #[schema(example = "подтекание топлива из дренажных колпачков")]
    #[allow(dead_code)]
    pub classifier_text: String,
    /// Human-readable note stored on the task.
    #[schema(example = "Обнаружены следы подтекания топлива")]
    #[allow(dead_code)]
    pub note: Option<String>,
    /// Previous name of `note`. Accepted for one release.
    #[serde(default)]
    #[allow(dead_code)]
    pub description: Option<String>,
    /// UUID of the aircraft.
    #[schema(format = "uuid", example = "3fa85f64-5717-4562-b3fc-2c963f66afa6")]
    #[allow(dead_code)]
    pub plane_uuid: String,
    /// Existing request in status `unassigned`.
    #[serde(default)]
    pub request_id: Option<String>,
    /// Engineer to assign by hand. When omitted the best eligible candidate wins.
    #[schema(format = "uuid", example = "3fa85f64-5717-4562-b3fc-2c963f66afa6")]
    #[serde(default)]
    pub engineer_id: Option<String>,
}

/// Request payload for `POST /api/assign/candidates`. Only the request being
/// ranked is needed: the preview reads nothing else and writes nothing, so the
/// fields an actual assignment reads (`classifier_text`, `plane_uuid`) have no
/// business being required here.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignCandidatesRequest {
    /// Existing request in a non-terminal status.
    #[schema(format = "uuid", example = "3fa85f64-5717-4562-b3fc-2c963f66afa6")]
    pub request_id: String,
}

/// Answer of `POST /api/assign/candidates`: the ranking a dispatcher reads
/// before choosing an engineer. Nothing is assigned by it.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssignmentCandidatesResponse {
    /// Request the ranking was built for.
    pub request_id: String,
    /// ATA chapter the engineers were checked against, when it could be resolved.
    #[schema(nullable)]
    pub chapter: Option<String>,
    /// Engineers ordered from best to worst; `eligible` tells who can take it.
    pub candidates: Vec<AssignmentCandidate>,
    /// Milliseconds spent building the ranking.
    pub candidate_ms: i32,
}

/// One aircraft currently stored in Redis.
#[derive(Debug, Serialize, ToSchema)]
pub struct AircraftEntry {
    /// Aircraft UUID.
    pub id: uuid::Uuid,
    /// Current position on the map.
    pub point: Point,
    /// When the aircraft arrived on the active map. Empty for a position that has no arrival record.
    #[schema(format = "date-time", nullable)]
    pub arrived_at: Option<String>,
}

/// Body of `POST /api/aircraft/arrive`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ArriveAircraftRequest {
    /// Aircraft UUID. Generated when omitted.
    #[schema(format = "uuid")]
    pub id: Option<String>,
    /// Stand position on the active map.
    pub point: Point,
    /// Arrival time. The server clock is used when omitted.
    #[schema(format = "date-time")]
    pub arrived_at: Option<String>,
}

/// Body of `POST /api/aircraft/depart`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DepartAircraftRequest {
    /// Aircraft UUID.
    #[schema(format = "uuid")]
    pub id: String,
    /// Departure time. The server clock is used when omitted.
    #[schema(format = "date-time")]
    pub departed_at: Option<String>,
}

/// One recorded stay of an aircraft on a map.
#[derive(Debug, Serialize, ToSchema)]
pub struct AircraftMovementResponse {
    /// Movement row id.
    pub id: uuid::Uuid,
    /// Aircraft UUID.
    pub aircraft_id: uuid::Uuid,
    /// Map the aircraft arrived on.
    pub map_id: String,
    /// Stand used at arrival.
    pub point: Point,
    /// When the aircraft arrived on the map.
    #[schema(format = "date-time")]
    pub arrived_at: String,
    /// When the aircraft left the map. `null` while it is still parked.
    #[schema(format = "date-time", nullable)]
    pub departed_at: Option<String>,
}

/// Arrival and departure records for the active map.
#[derive(Debug, Serialize, ToSchema)]
pub struct AircraftMovementListResponse {
    /// Recorded aircraft stays on the active map.
    pub movements: Vec<AircraftMovementResponse>,
    /// Number of rows before `limit` and `offset` are applied.
    pub total: u64,
}

/// Aircraft positions visible to a dispatcher.
#[derive(Debug, Serialize, ToSchema)]
pub struct AircraftListResponse {
    /// Aircraft currently present on the active map.
    pub aircraft: Vec<AircraftEntry>,
    /// Number of aircraft before `limit` and `offset` are applied.
    pub total: u64,
}

/// Response payload for the `assign_engineer` endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssignEngineerResponse {
    /// UUID of the engineer chosen for this task.
    pub engineer_uuid: String,
    /// Estimated travel time in seconds.
    #[schema(minimum = 0)]
    pub eta_seconds: u64,
    /// Whether the assignment exceeds the allowed SLA.
    pub sla_exceeded: bool,
    /// Route length in map cells.
    #[schema(minimum = 0)]
    pub distance: u64,
    /// Previous name of `eta_seconds`. Kept for one release.
    #[schema(deprecated)]
    pub time: u64,
    /// Previous name of `sla_exceeded`. Kept for one release.
    #[schema(deprecated)]
    pub time_limit_exceeded: bool,
    /// Engineers considered for the task, with the reason for each verdict.
    pub candidates: Vec<AssignmentCandidate>,
    /// Milliseconds spent building the candidate list.
    pub candidate_ms: i32,
    /// Route the engineer must follow to reach the aircraft.
    pub route: Route,
}

/// One row of the assignment comparison.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssignmentCandidate {
    /// UUID of the engineer.
    pub engineer_id: String,
    /// Whether this engineer can take the task.
    pub eligible: bool,
    /// Why the engineer was accepted or rejected.
    pub reason: String,
    /// Estimated travel time in seconds.
    pub eta_seconds: u64,
    /// Whether the route requires a special vehicle.
    pub uses_vehicle: bool,
    /// SLA penalty in seconds; zero unless the travel time exceeds the SLA.
    pub late_seconds: f64,
    /// Workload penalty in seconds (open tasks times the per-task weight).
    pub load_penalty: f64,
    /// Seconds taken off for a grounded (AOG) aircraft; zero otherwise.
    pub aog_bonus: f64,
    /// Seconds taken off for a freshly arrived aircraft; fades over an hour.
    pub arrival_bonus: f64,
    /// Seconds taken off because the request is `urgent`; zero for `normal`.
    pub urgency_bonus: f64,
    /// Seconds taken off for the declared MEL category (A > B > C > D).
    pub mel_bonus: f64,
    /// The score as signed addends, in score order. Their sum equals `score`,
    /// so every point of the ranking is explainable.
    pub factors: Vec<ScoreFactor>,
    /// Final ranking score: `eta_seconds + late_seconds + load_penalty -
    /// aog_bonus - arrival_bonus - urgency_bonus - mel_bonus`. Lower is better;
    /// `f64::MAX` for a rejection.
    pub score: f64,
}

/// One addend of [`AssignmentCandidate::score`].
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ScoreFactor {
    /// Stable machine name, for example `eta`, `late`, `load` or `aog_bonus`.
    pub name: String,
    /// Contribution to the score in seconds: positive is a penalty, negative a
    /// bonus.
    pub seconds: f64,
}

/// Numeric justification of the engineer a request was assigned to, stored with
/// the task and handed to that engineer when the assignment is read or accepted.
///
/// The sum of [`Self::factors`] equals [`Self::score`], so the engineer sees the
/// same numbers the dispatcher's comparison showed.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SelectionBasis {
    /// UUID of the engineer the request was assigned to.
    pub engineer_id: String,
    /// Final ranking score that won; lower is better.
    pub score: f64,
    /// Estimated travel time in seconds at the moment of assignment.
    #[schema(minimum = 0)]
    pub eta_seconds: u64,
    /// Route length in map cells.
    #[schema(minimum = 0)]
    pub distance: u64,
    /// Whether the route requires a special vehicle.
    pub uses_vehicle: bool,
    /// Signed addends of [`Self::score`], in score order.
    pub factors: Vec<ScoreFactor>,
    /// Human-readable summary of the same facts.
    pub reason: String,
    /// True when a dispatcher named the engineer instead of taking the best row.
    pub manual: bool,
    /// When the assignment was made, ISO 8601 in UTC.
    pub chosen_at: String,
}

/// Answer of `POST /api/tasks/current/accept`: the new status and the numeric
/// justification of why this engineer was chosen for the request.
#[derive(Debug, Serialize, ToSchema)]
pub struct AcceptCurrentTaskResponse {
    /// Task status after accepting, always `in_progress`.
    pub status: String,
    /// Why this engineer was chosen. Absent only for tasks assigned before the
    /// field existed.
    #[schema(nullable)]
    pub selection: Option<SelectionBasis>,
}

/// Response payload describing the engineer's current task.
#[derive(Debug, Serialize, ToSchema)]
pub struct CurrentTaskResponse {
    /// Unique task identifier.
    pub id: String,
    /// Human-readable task description.
    pub description: String,
    /// Problem description stored with the task.
    pub issue: String,
    /// Location of the aircraft on the map at the time of assignment.
    pub plane_point: Point,
    /// UUID of the aircraft attached to this task.
    pub plane_uuid: Option<String>,
    /// Route from the engineer to the aircraft.
    pub route: Route,
    /// Current task status, for example `assigned` or `in_progress`.
    pub status: String,
    /// Estimated travel time in seconds.
    pub eta_seconds: u64,
    /// ATA chapter the issue belongs to, when known.
    #[schema(nullable)]
    pub ata_chapter: Option<String>,
    /// MEL category that applies to the issue, when known.
    #[schema(nullable)]
    pub mel_category: Option<String>,
    /// Whether the engineer has reached the aircraft.
    pub arrived: bool,
    /// Whether the engineer has collected the required tools.
    pub tools_picked: bool,
}

// ---------------------------------------------------------------------------
// User lookup
// ---------------------------------------------------------------------------

/// Predicted engineer role for a free-form problem description.
#[derive(Debug, Serialize, ToSchema)]
pub struct ClassifyResponse {
    /// Engineer specialization chosen by the TF-IDF Naive Bayes model.
    pub engineer_type: EngineerType,
    /// Probability of the chosen class.
    pub confidence: f64,
}

/// Result of retraining the problem classifier from the dataset on disk.
#[derive(Debug, Serialize, ToSchema)]
pub struct TrainClassifierResponse {
    /// Number of documents the model was trained on.
    pub documents: usize,
    /// When this training run finished.
    #[schema(format = "date-time", example = "2026-11-22T00:00:00Z")]
    pub trained_at: String,
    /// Held-out accuracy, when the trainer computed one.
    pub accuracy: Option<f64>,
}

/// Request payload for classifying a free-form problem description.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassifyRequest {
    /// Problem description in natural language.
    #[schema(example = "уход гироплатформы ИНС, курс смещается")]
    pub text: String,
}

/// Profile of the authenticated user. Never includes the password hash.
#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    /// User UUID.
    #[schema(format = "uuid")]
    pub id: String,
    /// Email of the authenticated user.
    #[schema(format = "email")]
    pub email: String,
    /// Display name of the authenticated user.
    pub name: String,
    /// Role granted to the account.
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
#[serde(deny_unknown_fields)]
pub struct RegisterRequest {
    /// User's email address (validated as an RFC 5322 email).
    #[validate(email)]
    #[schema(format = "email", example = "engineer@aeroflot.example")]
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

    /// Organiser roster id (`AT-0001` …) from `engineers_positions.geojson`.
    /// Optional: without it the account is gated by `engineer_type` alone.
    #[schema(example = "AT-0001")]
    pub tabel_id: Option<String>,
}

impl fmt::Debug for RegisterRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisterRequest")
            .field("email", &self.email)
            .field("name", &self.name)
            .field("password", &"<redacted>")
            .field("user_role", &self.user_role)
            .field("engineer_type", &self.engineer_type)
            .field("tabel_id", &self.tabel_id)
            .finish()
    }
}

/// Request payload for the `login` endpoint.
#[derive(Deserialize, ToSchema, Validate)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    /// User's email address.
    #[validate(email)]
    #[schema(format = "email", example = "engineer@aeroflot.example")]
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

/// OAuth token type. The API only issues bearer tokens.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub enum OAuthTokenType {
    /// Token sent in the `Authorization: Bearer <token>` header.
    Bearer,
}

/// Response payload for the `login` endpoint.
#[derive(Serialize, ToSchema)]
pub struct LoginResponse {
    /// User UUID.
    #[schema(format = "uuid")]
    pub id: String,
    /// Email of the authenticated user.
    #[schema(format = "email")]
    pub email: String,
    /// Display name of the authenticated user.
    pub name: String,
    /// Role of the authenticated user.
    pub user_role: UserRole,
    /// Engineer specialization. `null` unless `user_role` is `Engineer`.
    pub engineer_type: Option<EngineerType>,
    /// Short-lived access token.
    pub access_token: String,
    /// Long-lived refresh token.
    pub refresh_token: String,
    /// Always `Bearer`.
    pub token_type: OAuthTokenType,
    /// Access-token lifetime in seconds.
    #[schema(minimum = 0)]
    pub expires_in: i64,
}

impl fmt::Debug for LoginResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginResponse")
            .field("id", &self.id)
            .field("email", &self.email)
            .field("name", &self.name)
            .field("user_role", &self.user_role)
            .field("engineer_type", &self.engineer_type)
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .finish()
    }
}

/// Request payload for the `update_access_token` endpoint.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
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
    /// Rotated refresh token. The presented refresh token is no longer valid.
    pub refresh_token: String,
    /// Always `Bearer`.
    pub token_type: OAuthTokenType,
    /// Access-token lifetime in seconds.
    #[schema(minimum = 0)]
    pub expires_in: i64,
}

impl fmt::Debug for UpdateAccessResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateAccessResponse")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .finish()
    }
}

/// Request payload for setting or clearing AOG on the engineer's current task.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CurrentTaskAogRequest {
    /// `true` marks the task's aircraft as AOG, `false` clears the status.
    pub aog: Option<bool>,
    /// Previous name of `aog`. Accepted for one release.
    #[serde(default)]
    pub toggle: Option<bool>,
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
    /// Aircraft marked AOG, with the position known to the server.
    pub aircraft: Vec<AircraftEntry>,
    /// Number of AOG aircraft before `limit` and `offset` are applied.
    pub total: u64,
    /// Previous payload: UUIDs only. Kept for one release.
    #[schema(deprecated)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct UpdateTransportPositionRequest {
    /// UUID of the special vehicle.
    pub id: String,
    /// Type of the special vehicle.
    pub vehicle_type: VehicleType,
    /// New position on the map.
    pub new_point: Point,
}

/// Request payload for pinning a tracked object to a known cell.
///
/// Give the target either as a grid cell (`point`) or as WGS-84 (`lat` plus
/// `lon`), never both and never neither; a pair of degrees is converted with
/// the active map projection.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CalibrateObjectRequest {
    /// Object to pin: an engineer UUID, `RA-…` aircraft, or the `obj` a tracker
    /// reports.
    pub obj: String,
    /// Kind of object, which decides the surface the pin lives on.
    pub kind: ObjectKind,
    /// Cell to pin, `[x, y]`.
    pub point: Option<Point>,
    /// Latitude in degrees, converted with the active map projection.
    pub lat: Option<f64>,
    /// Longitude in degrees, converted with the active map projection.
    pub lon: Option<f64>,
}

/// Request payload for dropping an object's pin.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ClearCalibrationRequest {
    /// Object whose pin is removed; it snaps to raw GPS again afterwards.
    pub obj: String,
}

/// One stored pin, as returned by the calibration endpoints.
#[derive(Debug, Serialize, ToSchema)]
pub struct CalibrationEntry {
    /// The pinned object.
    pub obj: String,
    /// Kind of object.
    pub kind: ObjectKind,
    /// Pinned cell.
    pub point: Point,
    /// The pinned cell as WGS-84, so an operator can compare it with a phone.
    pub geo: Coordinate,
    /// False when the pinned cell carries no graph node, so routes cannot start
    /// or end on it and fall to the nearest node instead.
    pub on_graph: bool,
}

/// Pins currently stored for the active map.
#[derive(Debug, Serialize, ToSchema)]
pub struct CalibrationListResponse {
    pub calibrations: Vec<CalibrationEntry>,
    /// Number of pins before `limit` and `offset` are applied.
    pub total: u64,
}

/// A WGS-84 coordinate in degrees.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct Coordinate {
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
}

/// Request ids grouped by the aircraft they belong to.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlaneRequests {
    /// UUID of the aircraft.
    pub plane_id: String,
    /// Request ids attached to this aircraft.
    pub request_ids: Vec<String>,
}

/// All request ids keyed by aircraft UUID.
#[derive(Debug, Serialize, ToSchema)]
#[serde(transparent)]
pub struct AllRequests(
    /// Map of aircraft UUID to the request ids attached to it.
    pub BTreeMap<String, Vec<String>>,
);
