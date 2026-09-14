use utoipa::OpenApi;

use crate::router::{
    __path_assign_engineer, __path_get_map, __path_get_route,
    auth::{__path_login_handler, __path_register_handler, __path_update_access_token},
};
use crate::types::map::{MapMatrix, Point};

#[derive(OpenApi)]
#[openapi(
    paths(get_map, get_route, assign_engineer, login_handler, register_handler, update_access_token),
    components(schemas(
        MapMatrix,
        Point,
    )),
    tags(
        (name="Aeroflot", description="Aeroflot dispatcher management API")
    )
)]
pub struct ApiDoc;
