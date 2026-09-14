use crate::types::{
    enums::{AircraftIssue, EngineerType, UserRole},
    map::{MapMatrix, Point, Route},
};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

// Public handlers

#[derive(Debug, Serialize)]
pub struct MatrixResponse {
    pub width: usize,
    pub height: usize,
    pub matrix: MapMatrix,
}

impl MatrixResponse {
    pub fn new(matrix: MapMatrix) -> anyhow::Result<Self> {
        if matrix.0.is_empty() {
            return Err(anyhow!("got empty matrix".to_string()));
        }

        let width = matrix.0[0].len();
        let height = matrix.0.len();
        Ok(Self {
            matrix,
            width,
            height,
        })
    }
}

#[derive(Serialize, ToSchema)]
pub struct GetRouteResponse {
    pub route: Route,
    pub distance: usize,
}

#[derive(Deserialize, ToSchema)]
pub struct GetRouteRequest {
    pub start_point: Point,
    pub end_point: Point,
}

#[derive(Deserialize, ToSchema)]
pub struct AssignEngineerRequest {
    pub issue: AircraftIssue,
    pub plane_point: Point,
    pub description: String,
}

#[derive(Serialize, ToSchema)]
pub struct AssignEngineerResponse {
    pub engineer_uuid: String,
    pub time: u64,
    pub time_limit_exceeded: bool,
    pub route: Route,
}

#[derive(Deserialize, ToSchema)]
pub struct GetUserNameRequest {
    pub id: String,
}

#[derive(Serialize, ToSchema)]
pub struct GetUserNameResponse {
    pub name: String,
}

// auth handlers

#[derive(Deserialize, ToSchema, Validate)]
pub struct RegisterRequest {
    #[validate(email)]
    pub email: String,
    pub name: String,
    pub password: String,
    pub user_role: UserRole,
    pub engineer_type: Option<EngineerType>,
}

#[derive(Deserialize, ToSchema, Validate)]
pub struct LoginRequest {
    #[validate(email)]
    pub email: String,
    pub password: String,
}

#[derive(Serialize, ToSchema)]
pub struct LoginResponse {
    pub name: String,
    pub user_role: UserRole,
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateAccessRequest {
    pub refresh_token: String,
}

#[derive(Serialize, ToSchema)]
pub struct UpdateAccessResponse {
    pub access_token: String,
}

// simulation handlers

#[derive(Deserialize, ToSchema)]
pub struct UpdateEngineerPositionRequest {
    pub id: String,
    pub new_point: Point,
}
