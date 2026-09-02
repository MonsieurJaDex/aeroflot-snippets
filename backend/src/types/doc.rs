use utoipa::OpenApi;

use crate::router::__path_get_map;
use crate::types::map::MapMatrix;

#[derive(OpenApi)]
#[openapi(
    paths(get_map),
    components(schemas(
        MapMatrix<u8>
    )),
    tags(
        (name="Aeroflot", description="Aeroflot dispatcher management API")
    )
)]
pub struct ApiDoc;
