use diesel_derive_enum::DbEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, DbEnum)]
#[serde(rename_all = "snake_case")]
#[ExistingTypePath = "crate::database::schema::sql_types::EngineerType"]
#[DbValueStyle = "snake_case"]
pub enum EngineerType {
    IntegrityInspector, // проверка целостности и герметичности систем
    CrewRemarksHandler, // устранение замечаний экипажа
    FuelingCrew,        // заправка (топливо, масло, кислород)
    EngineTechnician,   // техник по двигателю
    AvionicsEngineer,   // инженер по радиоэлектронному оборудованию
    AviationTechnician, // диагностика, дефектация, регулировка, ремонт
}

#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash, Serialize, Deserialize, DbEnum)]
#[serde(rename_all = "snake_case")]
#[ExistingTypePath = "crate::database::schema::sql_types::AircraftIssue"]
#[DbValueStyle = "snake_case"]
pub enum AircraftIssue {
    // Герметичность систем
    FuelLeakFromDrainCap, // подтекание топлива из дренажных колпачков
    OilStainNearGearbox,  // масляные пятна в районе редуктора
    HydraulicLeakOnStrut, // следы гидравлики на штоках амортизаторов шасси

    // Внешние повреждения
    FairingChipOrScratch, // сколы и царапины на обтекателях/антеннах/фонарях
    PaintPeelingAtRivets, // отслоение краски в зонах клёпки
    MissingPitotCover,    // отсутствие заглушек на приёмниках давления

    // Пневматики шасси
    UnevenTreadWear, // неравномерный износ протектора
    TireCutToCord,   // порезы до корда
    LowTirePressure, // низкое давление в шинах

    // Замечания экипажа
    IndicationFault,     // сбои индикации (лампа, предохранитель)
    LooseConnector,      // ослабленный разъём
    SeatbeltAdjustment,  // регулировка привязных ремней
    BurnedOutSignalLamp, // перегоревшая светосигнальная лампа

    // Двигатель
    ThrustOrParameterDrop,  // падение тяги/оборотов/температуры газов
    ExcessiveVibration,     // повышенная вибрация (дисбаланс)
    MetalDebrisInOilFilter, // стружка в маслофильтре

    // Радиоэлектронное оборудование
    RadarFailureOrFalseReading, // отказ/ложные показания РЛС
    CommsLossOrDistortion,      // потеря связи / искажение сигнала
    InsGyroDrift,               // уход гироплатформы ИНС
}

impl AircraftIssue {
    /// Evaluate sp
    pub fn responsible_engineer(&self) -> EngineerType {
        use AircraftIssue::*;
        match self {
            FuelLeakFromDrainCap | OilStainNearGearbox | HydraulicLeakOnStrut
            | FairingChipOrScratch | PaintPeelingAtRivets | MissingPitotCover | UnevenTreadWear
            | TireCutToCord | LowTirePressure => EngineerType::IntegrityInspector,

            IndicationFault | LooseConnector | SeatbeltAdjustment | BurnedOutSignalLamp => {
                EngineerType::CrewRemarksHandler
            }

            ThrustOrParameterDrop | ExcessiveVibration | MetalDebrisInOilFilter => {
                EngineerType::EngineTechnician
            }

            RadarFailureOrFalseReading | CommsLossOrDistortion | InsGyroDrift => {
                EngineerType::AvionicsEngineer
            }
        }
    }
}
