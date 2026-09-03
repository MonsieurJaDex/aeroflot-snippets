use num_traits::PrimInt;

use crate::types::{enums::EngineerType, map::Point};

pub struct Engineer<T>
where
    T: PrimInt,
{
    pub id: String,
    pub engineer_type: EngineerType,
    pub position: Point<T>,
    // is assigned
}
