//! Aggregated OpenAPI (utoipa) documentation.
//!
//! Paths are registered by their `#[utoipa::path]` handler functions; all
//! public DTOs, enums and domain types referenced by them are listed in
//! [`components`] so the generated schema is exhaustive.

use utoipa::openapi::schema::{
    AdditionalProperties, ArrayBuilder, ArrayItems, Discriminator, Object, ObjectBuilder,
    OneOfBuilder, Ref, Schema, Type,
};
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::openapi::{Deprecated, RefOr};
use utoipa::{Modify, OpenApi};

use crate::router::{
    __path_arrive_aircraft, __path_assign_candidates, __path_assign_engineer,
    __path_classify_problem, __path_complete_current_task, __path_current_task_aog,
    __path_current_task_aog_post, __path_depart_aircraft, __path_get_active_map,
    __path_get_current_task, __path_get_map, __path_get_route, __path_list_aircraft,
    __path_list_aircraft_movements, __path_list_aog, __path_list_maps, __path_set_active_map,
    __path_train_classifier,
    auth::{
        __path_get_engineer_name_handler, __path_login_handler, __path_me_handler,
        __path_register_handler, __path_update_access_token,
    },
    geo::{
        __path_active_engineers, __path_calibrate_object_handler,
        __path_clear_object_calibration_handler, __path_get_engineers_positions,
        __path_get_object_calibrations, __path_get_transport_positions,
        __path_update_engineer_position_handler, __path_update_transport_position_handler,
        ActiveEngineersResponse, EngineerPositionEntry, EngineersPositionsResponse,
        TransportPositionEntry, TransportPositionsResponse,
    },
    ops::{
        __path_accept_current, __path_create_defect, __path_create_request, __path_export_journal,
        __path_list_alerts, __path_pick_tools, __path_queue_request, __path_reassign_request,
        __path_refresh_route, __path_reject_current, __path_set_mel, __path_unassign_request,
        __path_wait_for_part, AlertItem, AlertList, CreateRequest, MelRequest, NewDefect,
        ReassignRequest, RequestCreated,
    },
    track::{__path_ingest, __path_ingest_query, TrackAck, TrackBatch, TrackPoint},
};
use crate::types::{
    dto::{
        ActiveMapResponse, AircraftEntry, AircraftListResponse, AircraftMovementListResponse,
        AircraftMovementResponse, AogListResponse, ArriveAircraftRequest, AssignEngineerRequest,
        AssignCandidatesRequest, AssignEngineerResponse, AssignmentCandidate,
        AssignmentCandidatesResponse,
        CalibrateObjectRequest, CalibrationEntry, CalibrationListResponse, ClassifyRequest,
        ClassifyResponse, ClearCalibrationRequest, Coordinate, CurrentTaskAogRequest,
        CurrentTaskAogResponse, CurrentTaskResponse, DepartAircraftRequest, EngineerPositionSample,
        ErrorResponse, GetRouteRequest, GetRouteResponse, GetUserNameResponse, LoginRequest,
        LoginResponse, MapListResponse, MapResponse, MapSummary, MeResponse, OAuthTokenType,
        RegisterRequest, RouteAgent, ScoreFactor, SelectMapRequest, TrainClassifierResponse,
        UpdateAccessRequest, UpdateAccessResponse, UpdateEngineerPositionRequest,
        UpdateTransportPositionRequest,
    },
    enums::{AircraftIssue, EngineerType, ObjectKind, UserRole, VehicleType},
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
                       - Automatic engineer assignment from a free-text problem description.\n\
                       - Task acceptance / progress tracking.\n\
                       - Geo endpoints for engineers, aircraft and special transport vehicles.\n\
                       - JWT-based authentication (access + refresh tokens).",
        contact(name = "Aeroflot Backend Team"),
    ),
    modifiers(&SpecPolish),
    paths(
        get_map,
        list_maps,
        get_active_map,
        set_active_map,
        get_route,
        assign_engineer,
        assign_candidates,
        classify_problem,
        train_classifier,
        list_aircraft,
        arrive_aircraft,
        depart_aircraft,
        list_aircraft_movements,
        create_request,
        queue_request,
        reassign_request,
        accept_current,
        reject_current,
        pick_tools,
        set_mel,
        wait_for_part,
        refresh_route,
        unassign_request,
        list_alerts,
        create_defect,
        export_journal,
        ingest,
        ingest_query,
        get_current_task,
        current_task_aog,
        current_task_aog_post,
        list_aog,
        complete_current_task,
        login_handler,
        register_handler,
        me_handler,
        update_access_token,
        get_engineer_name_handler,
        get_engineers_positions,
        update_engineer_position_handler,
        active_engineers,
        get_transport_positions,
        update_transport_position_handler,
        calibrate_object_handler,
        clear_object_calibration_handler,
        get_object_calibrations,
    ),
    components(schemas(
        // --- Domain types ---
        MapMatrix,
        MapResponse,
        MapSummary,
        MapListResponse,
        ActiveMapResponse,
        SelectMapRequest,
        Point,
        Route,
        ErrorResponse,
        // --- Enums ---
        AircraftIssue,
        EngineerType,
        VehicleType,
        UserRole,
        ObjectKind,
        // --- DTOs: routes ---
        GetRouteRequest,
        GetRouteResponse,
        RouteAgent,
        // --- DTOs: assignment ---
        AssignmentCandidate,
        ScoreFactor,
        AssignEngineerRequest,
        AssignCandidatesRequest,
        AssignEngineerResponse,
        AssignmentCandidatesResponse,
        ClassifyRequest,
        ClassifyResponse,
        TrainClassifierResponse,
        AircraftEntry,
        AircraftListResponse,
        ArriveAircraftRequest,
        DepartAircraftRequest,
        AircraftMovementResponse,
        AircraftMovementListResponse,
        CurrentTaskResponse,
        CurrentTaskAogRequest,
        CurrentTaskAogResponse,
        AogListResponse,
        // --- DTOs: user lookup ---
        GetUserNameResponse,
        // --- DTOs: auth ---
        RegisterRequest,
        LoginRequest,
        LoginResponse,
        OAuthTokenType,
        MeResponse,
        UpdateAccessRequest,
        UpdateAccessResponse,
        // --- DTOs: geo ---
        UpdateEngineerPositionRequest,
        EngineerPositionSample,
        ActiveEngineersResponse,
        EngineerPositionEntry,
        EngineersPositionsResponse,
        TransportPositionEntry,
        TransportPositionsResponse,
        UpdateTransportPositionRequest,
        CalibrateObjectRequest,
        ClearCalibrationRequest,
        CalibrationEntry,
        CalibrationListResponse,
        Coordinate,
        TrackPoint,
        TrackBatch,
        TrackAck,
        CreateRequest,
        RequestCreated,
        ReassignRequest,
        MelRequest,
        AlertItem,
        AlertList,
        NewDefect,
    )),
    tags(
        (name = "Auth", description = "Registration, login and the current profile"),
        (name = "Aircraft", description = "Aircraft arrivals, departures, positions and AOG list"),
        (name = "Assign", description = "Assign an engineer to an aircraft"),
        (name = "Classifier", description = "Problem-text classification"),
        (name = "Geo", description = "Live positions of engineers and vehicles"),
        (name = "Route", description = "Point-to-point routes"),
        (name = "Map", description = "Airport map"),
        (name = "Tasks", description = "The authenticated engineer's current task")
    )
)]
pub struct ApiDoc;

/// Tightens generated schemas: bearer auth, closed request objects, register oneOf.
struct SpecPolish;

impl Modify for SpecPolish {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let Some(components) = openapi.components.as_mut() else {
            return;
        };
        components.add_security_scheme(
            "bearerAuth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );

        for name in [
            "AssignEngineerRequest",
            "ClassifyRequest",
            "LoginRequest",
            "GetRouteRequest",
            "UpdateAccessRequest",
            "UpdateEngineerPositionRequest",
            "UpdateTransportPositionRequest",
            "CurrentTaskAogRequest",
        ] {
            if let Some(object) = schema_object(components, name) {
                object.additional_properties =
                    Some(Box::new(AdditionalProperties::FreeForm(false)));
            }
        }
        deprecate_property(components, "AssignEngineerResponse", "time");
        deprecate_property(components, "AssignEngineerResponse", "time_limit_exceeded");
        deprecate_property(components, "AssignEngineerRequest", "description");
        deprecate_property(components, "CurrentTaskAogRequest", "toggle");
        deprecate_property(components, "AogListResponse", "plane_uuids");
        if let Some(object) = schema_object(components, "ErrorResponse") {
            if let Some(RefOr::T(Schema::Object(details))) = object.properties.get_mut("details") {
                details.additional_properties =
                    Some(Box::new(AdditionalProperties::FreeForm(true)));
            }
        }
        if let Some(object) = schema_object(components, "TrainClassifierResponse") {
            if !object.required.iter().any(|field| field == "accuracy") {
                object.required.push("accuracy".to_owned());
            }
        }
        if let Some(object) = schema_object(components, "CurrentTaskAogRequest") {
            if !object.required.iter().any(|field| field == "aog") {
                object.required.push("aog".to_owned());
            }
        }
        if let Some(object) = schema_object(components, "AssignEngineerRequest") {
            if !object.required.iter().any(|field| field == "note") {
                object.required.push("note".to_owned());
            }
        }

        components.schemas.insert(
            "RegisterEngineer".to_owned(),
            role_register_schema("Engineer", true).into(),
        );
        components.schemas.insert(
            "RegisterDispatcher".to_owned(),
            role_register_schema("Dispatcher", false).into(),
        );
        let mut discriminator = Discriminator::new("user_role");
        discriminator.mapping.insert(
            "Engineer".to_owned(),
            "#/components/schemas/RegisterEngineer".to_owned(),
        );
        discriminator.mapping.insert(
            "Dispatcher".to_owned(),
            "#/components/schemas/RegisterDispatcher".to_owned(),
        );
        components.schemas.insert(
            "RegisterRequest".to_owned(),
            OneOfBuilder::new()
                .item(Ref::from_schema_name("RegisterEngineer"))
                .item(Ref::from_schema_name("RegisterDispatcher"))
                .discriminator(Some(discriminator))
                .description(Some(
                    "Registration body. engineer_type is required only for Engineer.",
                ))
                .into(),
        );

        components.schemas.insert(
            "EngineerPositionSample".to_owned(),
            ArrayBuilder::new()
                .schema_type(Type::Array)
                .items(ArrayItems::False)
                .prefix_items([
                    Schema::Array(
                        ArrayBuilder::new()
                            .items(Object::with_type(Type::Integer))
                            .min_items(Some(2))
                            .max_items(Some(2))
                            .description(Some("Point [x, y]"))
                            .build(),
                    ),
                    Schema::Object(
                        ObjectBuilder::new()
                            .schema_type(Type::Integer)
                            .format(Some(utoipa::openapi::schema::SchemaFormat::KnownFormat(
                                utoipa::openapi::schema::KnownFormat::Int64,
                            )))
                            .description(Some("Unix timestamp in seconds"))
                            .build(),
                    ),
                ])
                .min_items(Some(2))
                .max_items(Some(2))
                .description(Some("[point, unix_timestamp_seconds]"))
                .build()
                .into(),
        );

        if let Some(path) = openapi.paths.paths.get_mut("/api/tasks/current/aog") {
            if let Some(post) = path.post.as_mut() {
                post.deprecated = Some(Deprecated::True);
            }
        }
    }
}

fn schema_object<'a>(
    components: &'a mut utoipa::openapi::Components,
    name: &str,
) -> Option<&'a mut Object> {
    match components.schemas.get_mut(name)? {
        RefOr::T(Schema::Object(object)) => Some(object),
        _ => None,
    }
}

fn deprecate_property(components: &mut utoipa::openapi::Components, schema: &str, field: &str) {
    let Some(object) = schema_object(components, schema) else {
        return;
    };
    if let Some(RefOr::T(Schema::Object(property))) = object.properties.get_mut(field) {
        property.deprecated = Some(Deprecated::True);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_openapi() {
        let mut value = serde_json::to_value(ApiDoc::openapi()).expect("openapi serializes");
        value["components"]["schemas"]["EngineerPositionSample"] = serde_json::json!({
            "description": "[point, unix_timestamp_seconds]",
            "type": "array",
            "prefixItems": [
                { "$ref": "#/components/schemas/Point" },
                { "type": "integer", "format": "int64" }
            ],
            "minItems": 2,
            "maxItems": 2,
            "items": false
        });
        let json = serde_json::to_string_pretty(&value).expect("openapi pretty");
        std::fs::write("openapi.json", json).expect("write openapi.json");
    }
}

fn role_register_schema(role: &str, engineer: bool) -> Object {
    let mut builder = ObjectBuilder::new()
        .property(
            "email",
            ObjectBuilder::new().schema_type(Type::String).format(Some(
                utoipa::openapi::schema::SchemaFormat::KnownFormat(
                    utoipa::openapi::KnownFormat::Email,
                ),
            )),
        )
        .property("name", ObjectBuilder::new().schema_type(Type::String))
        .property("password", ObjectBuilder::new().schema_type(Type::String))
        .property(
            "user_role",
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some([role])),
        )
        .required("email")
        .required("name")
        .required("password")
        .required("user_role")
        .additional_properties(Some(AdditionalProperties::FreeForm(false)))
        .description(Some(format!("Register a {role}")));
    if engineer {
        builder = builder
            .property("engineer_type", Ref::from_schema_name("EngineerType"))
            .required("engineer_type");
    }
    builder.build()
}
