ALTER TABLE tasks
    ALTER COLUMN assigned_engineer DROP NOT NULL;

ALTER TABLE tasks DROP CONSTRAINT IF EXISTS tasks_created_by_fkey;

ALTER TABLE tasks
    ADD COLUMN status TEXT NOT NULL DEFAULT 'assigned',
    ADD COLUMN recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN ata_chapter TEXT,
    ADD COLUMN defect_text TEXT,
    ADD COLUMN mel_category TEXT,
    ADD COLUMN mel_defer_until TIMESTAMPTZ,
    ADD COLUMN waiting_part BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN tools_picked BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN arrived_at TIMESTAMPTZ,
    ADD COLUMN source TEXT,
    ADD COLUMN urgency TEXT NOT NULL DEFAULT 'normal',
    ADD COLUMN eta_seconds INTEGER,
    ADD COLUMN candidate_ms INTEGER,
    ADD COLUMN sla_alerted BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN sla_breached BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE journal (
    id UUID PRIMARY KEY,
    at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    kind TEXT NOT NULL,
    actor TEXT,
    task_id UUID,
    params TEXT NOT NULL DEFAULT '{}',
    duration_ms INTEGER
);

CREATE TABLE defect_types (
    id UUID PRIMARY KEY,
    body TEXT NOT NULL,
    ata_chapter TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
