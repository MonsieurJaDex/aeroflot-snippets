use std::{fmt, time::Duration};

use diesel_derive_enum::DbEnum;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::models::{dispatcher::Dispatcher, engineer::Engineer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, DbEnum, ToSchema)]
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
    NotCagegorized,     // неопределен
}

#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash, Serialize, Deserialize, DbEnum, ToSchema)]
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

    // Дополнительно
    Other, // другое
}

impl AircraftIssue {
    /// Evaluate responsible engineer
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
            Other => EngineerType::NotCagegorized,
        }
    }

    pub fn resolution_time(&self) -> Duration {
        use AircraftIssue::*;
        match self {
            // Мелкие визуальные/некритичные замечания — минуты
            MissingPitotCover => Duration::from_mins(5),
            BurnedOutSignalLamp => Duration::from_mins(10),
            LooseConnector => Duration::from_mins(15),
            SeatbeltAdjustment => Duration::from_mins(10),
            IndicationFault => Duration::from_mins(20),
            FairingChipOrScratch => Duration::from_mins(20),
            PaintPeelingAtRivets => Duration::from_mins(30),

            // Требуют осмотра/незначительного ремонта — до часа
            LowTirePressure => Duration::from_mins(20),
            UnevenTreadWear => Duration::from_mins(45),
            OilStainNearGearbox => Duration::from_mins(45),
            FuelLeakFromDrainCap => Duration::from_mins(30),

            // Требуют замены узла/детальной диагностики — 1–3 часа
            TireCutToCord => Duration::from_mins(90), // замена колеса
            HydraulicLeakOnStrut => Duration::from_mins(120),
            RadarFailureOrFalseReading => Duration::from_mins(120),
            CommsLossOrDistortion => Duration::from_mins(90),
            InsGyroDrift => Duration::from_mins(180),

            // Серьёзные, требуют глубокой диагностики/разборки — часы
            ThrustOrParameterDrop => Duration::from_mins(240),
            ExcessiveVibration => Duration::from_mins(240),
            MetalDebrisInOilFilter => Duration::from_mins(360), // возможна разборка двигателя

            // Неопределённая категория
            Other => Duration::from_mins(120),
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema, Clone, PartialEq, Eq)]
pub enum UserRole {
    Dispatcher,
    Engineer,
}

impl fmt::Display for UserRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UserRole::Dispatcher => f.write_str("dispatcher"),
            UserRole::Engineer => f.write_str("engineer"),
        }
    }
}

impl TryFrom<String> for UserRole {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let value = value.trim().to_lowercase();
        match value.as_str() {
            "engineer" => Ok(Self::Engineer),
            "dispatcher" => Ok(Self::Dispatcher),
            _ => Err(anyhow::anyhow!(
                "failed to parse String to UserRole: expected 'Engineer' or 'Dispatcher', got: {}",
                value
            )),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq)]
pub enum TokenType {
    Access,
    Refresh,
}

pub enum AuthenticatedUser {
    Engineer(Engineer),
    Dispatcher(Dispatcher),
}

impl AuthenticatedUser {
    pub fn id(&self) -> Uuid {
        match self {
            AuthenticatedUser::Engineer(e) => e.id,
            AuthenticatedUser::Dispatcher(d) => d.id,
        }
    }

    pub fn password_hash(&self) -> &str {
        match self {
            AuthenticatedUser::Engineer(e) => &e.password_hash,
            AuthenticatedUser::Dispatcher(d) => &d.password_hash,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            AuthenticatedUser::Engineer(engineer) => &engineer.name,
            AuthenticatedUser::Dispatcher(dispatcher) => &dispatcher.name,
        }
    }

    pub fn role(&self) -> UserRole {
        match self {
            AuthenticatedUser::Engineer(_) => UserRole::Engineer,
            AuthenticatedUser::Dispatcher(_) => UserRole::Dispatcher,
        }
    }
}
