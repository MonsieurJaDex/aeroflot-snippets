package types

import "time"

// IssueCategory — категория неисправности
// (ТЗ: классификация по направлениям/категориям, авиационные главы).
type IssueCategory string

const (
	CategorySeal        IssueCategory = "seal"         // герметичность
	CategoryExternal    IssueCategory = "external"     // внешние повреждения
	CategoryLandingGear IssueCategory = "landing_gear" // шасси/пневматики
	CategoryCrew        IssueCategory = "crew"         // замечания экипажа
	CategoryEngine      IssueCategory = "engine"
	CategoryAvionics    IssueCategory = "avionics"
	CategoryOther       IssueCategory = "other"
)

// IssueType — код неисправности. Список НЕ закрытый
// (ТЗ: ввод текстом, добавление новых типов).
type IssueType string

// Предустановленные коды (совпадают с Rust AircraftIssue).
const (
	IssueFuelLeakFromDrainCap IssueType = "fuel_leak_from_drain_cap"
	IssueOilStainNearGearbox  IssueType = "oil_stain_near_gearbox"
	IssueHydraulicLeakOnStrut IssueType = "hydraulic_leak_on_strut"

	IssueFairingChipOrScratch IssueType = "fairing_chip_or_scratch"
	IssuePaintPeelingAtRivets IssueType = "paint_peeling_at_rivets"
	IssueMissingPitotCover    IssueType = "missing_pitot_cover"

	IssueUnevenTreadWear IssueType = "uneven_tread_wear"
	IssueTireCutToCord   IssueType = "tire_cut_to_cord"
	IssueLowTirePressure IssueType = "low_tire_pressure"

	IssueIndicationFault     IssueType = "indication_fault"
	IssueLooseConnector      IssueType = "loose_connector"
	IssueSeatbeltAdjustment  IssueType = "seatbelt_adjustment"
	IssueBurnedOutSignalLamp IssueType = "burned_out_signal_lamp"

	IssueThrustOrParameterDrop  IssueType = "thrust_or_parameter_drop"
	IssueExcessiveVibration     IssueType = "excessive_vibration"
	IssueMetalDebrisInOilFilter IssueType = "metal_debris_in_oil_filter"

	IssueRadarFailureOrFalseReading IssueType = "radar_failure_or_false_reading"
	IssueCommsLossOrDistortion      IssueType = "comms_loss_or_distortion"
	IssueInsGyroDrift               IssueType = "ins_gyro_drift"

	IssueOther IssueType = "other"
)

// Issue — заявка о неисправности. Может прийти от пилота/экипажа,
// диспетчера, внешней системы или инженера.
type Issue struct {
	ID          string        `json:"id"`
	Type        IssueType     `json:"type"`        // предустановленный или кастомный
	Category    IssueCategory `json:"category"`    // классификация
	Description string        `json:"description"` // свободный текст
	Source      ReportSource  `json:"source"`
	ReportedBy  string        `json:"reported_by"`
	PlaneID     string        `json:"plane_id"`
	PlanePoint  Point         `json:"plane_point"`
	Status      IssueStatus   `json:"status"`
	CreatedAt   time.Time     `json:"created_at"`
}

// IssueCatalog — справочник неисправностей (расширяемый).
type IssueCatalog interface {
	ByType(t IssueType) (IssueMeta, bool)
	ByCategory(c IssueCategory) []IssueMeta
	All() []IssueMeta
	Add(meta IssueMeta) error // добавление нового типа
	Search(query string) []IssueMeta
}

// IssueMeta — метаданные типа неисправности.
type IssueMeta struct {
	Type                IssueType
	Category            IssueCategory
	Title               string
	ResponsibleEngineer EngineerType
	RequiredVehicle     *SpecialVehicle
	ResolutionTime      time.Duration
}
