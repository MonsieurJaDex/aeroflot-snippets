use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};
use num_traits::PrimInt;
use serde::Serialize;

use crate::types::MapMatrix;

pub async fn get_map<T: PrimInt + Serialize>(State(map): State<Arc<MapMatrix<T>>>) -> Response {
    Json(&*map).into_response()
}
