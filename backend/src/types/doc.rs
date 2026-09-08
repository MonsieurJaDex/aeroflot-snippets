use utoipa::OpenApi;

use crate::router::{__path_assign_engineer, __path_get_map, __path_get_route};
use crate::types::map::{MapMatrix, Point};

#[derive(OpenApi)]
#[openapi(
    paths(get_map, get_route, assign_engineer),
    components(schemas(
        MapMatrix,
        Point,
    )),
    tags(
        (name="Aeroflot", description="Aeroflot dispatcher management API")
    )
)]
pub struct ApiDoc;
