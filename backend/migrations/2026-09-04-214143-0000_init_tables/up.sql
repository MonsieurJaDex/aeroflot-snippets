CREATE TYPE engineer_type AS ENUM (
    'integrity_inspector',
    'crew_remarks_handler',
    'fueling_crew',
    'engine_technician',
    'avionics_engineer',
    'aviation_technician'
);

CREATE TYPE aircraft_issue AS ENUM (
    'fuel_leak_from_drain_cap',
    'oil_stain_near_gearbox',
    'hydraulic_leak_on_strut',
    'fairing_chip_or_scratch',
    'paint_peeling_at_rivets',
    'missing_pitot_cover',
    'uneven_tread_wear',
    'tire_cut_to_cord',
    'low_tire_pressure',
    'indication_fault',
    'loose_connector',
    'seatbelt_adjustment',
    'burned_out_signal_lamp',
    'thrust_or_parameter_drop',
    'excessive_vibration',
    'metal_debris_in_oil_filter',
    'radar_failure_or_false_reading',
    'comms_loss_or_distortion',
    'ins_gyro_drift'
);

CREATE TABLE dispatchers (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          VARCHAR NOT NULL,
    email          VARCHAR NOT NULL UNIQUE,
    password_hash VARCHAR NOT NULL
);

CREATE TABLE engineers (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email          VARCHAR NOT NULL UNIQUE,
    engineer_type  engineer_type NOT NULL,
    password_hash  VARCHAR NOT NULL
);

CREATE TABLE tasks (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    ends_at            TIMESTAMPTZ NOT NULL,
    created_by         UUID NOT NULL REFERENCES dispatchers(id) ON DELETE RESTRICT,
    assigned_engineer  UUID NOT NULL REFERENCES engineers(id) ON DELETE RESTRICT,
    issue_type         aircraft_issue NOT NULL,
    is_active          BOOLEAN NOT NULL DEFAULT true
);

CREATE INDEX idx_tasks_assigned_engineer ON tasks(assigned_engineer);
CREATE INDEX idx_tasks_created_by ON tasks(created_by);
CREATE INDEX idx_tasks_is_active ON tasks(is_active) WHERE is_active = true;
CREATE INDEX idx_engineers_engineer_type ON engineers(engineer_type);
