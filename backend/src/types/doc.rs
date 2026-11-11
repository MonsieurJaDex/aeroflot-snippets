//! Aggregated OpenAPI (utoipa) documentation.
//!
//! Paths are registered by their `#[utoipa::path]` handler functions; all
//! public DTOs, enums and domain types referenced by them are listed in
//! [`components`] so the generated schema is exhaustive.

use utoipa::OpenApi;

use crate::router::{
    __path_accept_current_task, __path_assign_engineer, __path_get_current_task, __path_get_map,
    __path_get_route,
    auth::{
        __path_get_engineer_name_handler, __path_login_handler, __path_register_handler,
        __path_update_access_token,
    },
    simulate::{
        __path_active_engineers, __path_get_engineers_positions, __path_get_transport_positions,
        __path_update_engineer_position_handler, __path_update_transport_position_handler,
    },
};
use crate::types::{
    dto::{
        AssignEngineerRequest, AssignEngineerResponse, CurrentTaskResponse, GetRouteRequest,
        GetRouteResponse, GetUserNameRequest, GetUserNameResponse, LoginRequest, LoginResponse,
        RegisterRequest, UpdateAccessRequest, UpdateAccessResponse, UpdateEngineerPositionRequest,
        UpdateTransportPositionRequest,
    },
    enums::{AircraftIssue, EngineerType, SpecialVehicle, UserRole},
    map::{MapMatrix, Point, Route},
};

/// Root document of the API specification.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Aeroflot Dispatcher API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Backend API for dispatching aircraft maintenance engineers across \
                       the airport map.\n\n\
                       Features:\n\
                       - Route computation between any two map points.\n\
                       - Automatic engineer assignment based on the aircraft issue type.\n\
                       - Task acceptance / progress tracking.\n\
                       - Simulation endpoints for engineers and special transport vehicles.\n\
                       - JWT-based authentication (access + refresh tokens).",
        contact(name = "Aeroflot Backend Team"),
    ),
    paths(
        get_map,
        get_route,
        assign_engineer,
        get_current_task,
        accept_current_task,
        login_handler,
        register_handler,
        update_access_token,
        get_engineer_name_handler,
        get_engineers_positions,
        update_engineer_position_handler,
        active_engineers,
        get_transport_positions,
        update_transport_position_handler,
    ),
    components(schemas(
        // --- Domain types ---
        MapMatrix,
        Point,
        Route,
        // --- Enums ---
        AircraftIssue,
        EngineerType,
        SpecialVehicle,
        UserRole,
        // --- DTOs: routes ---
        GetRouteRequest,
        GetRouteResponse,
        // --- DTOs: assignment ---
        AssignEngineerRequest,
        AssignEngineerResponse,
        CurrentTaskResponse,
        // --- DTOs: user lookup ---
        GetUserNameRequest,
        GetUserNameResponse,
        // --- DTOs: auth ---
        RegisterRequest,
        LoginRequest,
        LoginResponse,
        UpdateAccessRequest,
        UpdateAccessResponse,
        // --- DTOs: simulation ---
        UpdateEngineerPositionRequest,
        UpdateTransportPositionRequest,
    )),
    tags(
        (name = "Aeroflot", description = "Aeroflot dispatcher management API")
    )
)]
pub struct ApiDoc;