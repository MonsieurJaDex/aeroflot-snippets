package types

// EngineerType — специализация инженера (см. Rust EngineerType).
type EngineerType string

const (
	EngineerIntegrityInspector EngineerType = "integrity_inspector"
	EngineerCrewRemarksHandler EngineerType = "crew_remarks_handler"
	EngineerFuelingCrew        EngineerType = "fueling_crew"
	EngineerEngineTechnician   EngineerType = "engine_technician"
	EngineerAvionics           EngineerType = "avionics_engineer"
	EngineerAviationTechnician EngineerType = "aviation_technician"
	EngineerNotCategorized     EngineerType = "not_categorized"
)

// SpecialVehicle — спецтехника (см. Rust SpecialVehicle).
type SpecialVehicle string

const (
	VehicleFuelTruck       SpecialVehicle = "fuel_truck"
	VehicleOilCart         SpecialVehicle = "oil_cart"
	VehicleMaintenanceLift SpecialVehicle = "maintenance_lift"
	VehicleBorescopeCart   SpecialVehicle = "borescope_cart"
	VehicleAvionicsTestSet SpecialVehicle = "avionics_test_set"
)

// UserRole — роль пользователя.
type UserRole string

const (
	RoleDispatcher UserRole = "dispatcher"
	RoleEngineer   UserRole = "engineer"
)

// IssueStatus — статус заявки.
type IssueStatus string

const (
	IssueStatusNew        IssueStatus = "new"
	IssueStatusTriaged    IssueStatus = "triaged"
	IssueStatusAssigned   IssueStatus = "assigned"
	IssueStatusInProgress IssueStatus = "in_progress"
	IssueStatusDone       IssueStatus = "done"
	IssueStatusCancelled  IssueStatus = "cancelled"
)

// TaskStatus — статус задачи инженера.
type TaskStatus string

const (
	TaskStatusAssigned   TaskStatus = "assigned"
	TaskStatusInProgress TaskStatus = "in_progress"
	TaskStatusPaused     TaskStatus = "paused"
	TaskStatusDone       TaskStatus = "done"
	TaskStatusCancelled  TaskStatus = "cancelled"
	TaskStatusReassigned TaskStatus = "reassigned"
)

// ReportSource — источник заявки (ТЗ: пилот/др. сотрудник через приложение/планшет).
type ReportSource string

const (
	SourcePilotApp       ReportSource = "pilot_app"
	SourceDispatcher     ReportSource = "dispatcher"
	SourceExternalSystem ReportSource = "external_system"
	SourceEngineer       ReportSource = "engineer"
)

// Priority — приоритет задачи (см. ТЗ: срочность, последствия задержки).
type Priority int

const (
	PriorityLow Priority = iota
	PriorityNormal
	PriorityHigh
	PriorityCritical
)

// AOGState — статус AOG (ТЗ: определяется инженером после осмотра).
type AOGState string

const (
	AOGNone      AOGState = "none"
	AOGSuspected AOGState = "suspected" // до осмотра
	AOGConfirmed AOGState = "confirmed" // инженер подтвердил AOG
	AOGRejected  AOGState = "rejected"  // инженер отклонил AOG
)
