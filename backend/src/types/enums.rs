use utoipa::ToSchema;

#[derive(ToSchema)]
pub enum EngineerType {
    // Персонал оперативного обслуживания
    OperationalMaintenance,

    // Техник по двигателю
    EngineTechnician,

    //  Инженер по радиоэлектронному оборудованию (Авионика)
    AvionicsEngineer,

    // Персонал периодического обслуживания и ремонта
    PeriodicMaintenanceTechnician,
}
