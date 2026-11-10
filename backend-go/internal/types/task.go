package types

import "time"

// Task — задача инженеру.
type Task struct {
	ID         string     `json:"id"`
	IssueID    string     `json:"issue_id"`
	AircraftID string     `json:"aircraft_id"`
	PlanePoint Point      `json:"plane_point"`
	AssignedTo string     `json:"assigned_to"` // uuid инженера
	Status     TaskStatus `json:"status"`
	Priority   Priority   `json:"priority"`

	// AOG (ТЗ: статус ставит инженер после осмотра).
	AOG      AOGState   `json:"aog"`
	AOGSetBy *string    `json:"aog_set_by,omitempty"`
	AOGSetAt *time.Time `json:"aog_set_at,omitempty"`

	// 15-минутное окно (ТЗ: строгое договорное условие).
	Deadline    time.Time  `json:"deadline"`
	ArrivedAt   *time.Time `json:"arrived_at,omitempty"`
	StartedAt   *time.Time `json:"started_at,omitempty"`
	CompletedAt *time.Time `json:"completed_at,omitempty"`

	Route Route         `json:"route,omitempty"`
	ETA   time.Duration `json:"eta,omitempty"`

	// Логика назначения (ТЗ: показывать, почему выбран инженер).
	Decision *AssignmentDecision `json:"decision,omitempty"`
}

// AssignmentDecision — обоснование выбора инженера.
// ТЗ: таблицы приоритетов, показывать логику решения.
type AssignmentDecision struct {
	EngineerUUID      string           `json:"engineer_uuid"`
	Reason            string           `json:"reason"`
	Candidates        []CandidateScore `json:"candidates"`
	TimeLimitExceeded bool             `json:"time_limit_exceeded"`
	GeneratedAt       time.Time        `json:"generated_at"`
}

// CandidateScore — оценка кандидата.
type CandidateScore struct {
	EngineerUUID string        `json:"engineer_uuid"`
	Score        int           `json:"score"`
	Rationale    string        `json:"rationale"`
	ETA          time.Duration `json:"eta"`
	Distance     float64       `json:"distance"`
}
