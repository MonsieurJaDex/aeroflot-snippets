use utoipa::OpenApi;

use crate::router::{
    __path_assign_engineer, __path_get_map, __path_get_route,
    auth::{
        __path_get_engineer_name_handler, __path_login_handler, __path_register_handler,
        __path_update_access_token,
    },
    simulate::{
        __path_active_engineers, __path_get_engineers_positions, __path_get_transport_positions,
        __path_update_engineer_position_handler, __path_update_transport_position_handler,
    },
};
use crate::types::map::{MapMatrix, Point};

#[derive(OpenApi)]
#[openapi(
    paths(get_map, get_route, assign_engineer, login_handler, register_handler, update_access_token, get_engineer_name_handler, get_engineers_positions, update_engineer_position_handler, active_engineers, get_transport_positions, update_transport_position_handler),
    components(schemas(
        MapMatrix,
        Point,
    )),
    tags(
        (name="Aeroflot", description="Aeroflot dispatcher management API")
    )
)]
pub struct ApiDoc;
