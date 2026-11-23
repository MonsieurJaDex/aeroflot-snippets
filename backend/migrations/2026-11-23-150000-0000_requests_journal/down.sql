DROP TABLE defect_types;
DROP TABLE journal;

ALTER TABLE tasks
    DROP COLUMN sla_breached,
    DROP COLUMN sla_alerted,
    DROP COLUMN candidate_ms,
    DROP COLUMN eta_seconds,
    DROP COLUMN urgency,
    DROP COLUMN source,
    DROP COLUMN arrived_at,
    DROP COLUMN tools_picked,
    DROP COLUMN waiting_part,
    DROP COLUMN mel_defer_until,
    DROP COLUMN mel_category,
    DROP COLUMN defect_text,
    DROP COLUMN ata_chapter,
    DROP COLUMN recorded_at,
    DROP COLUMN status;

ALTER TABLE tasks
    ALTER COLUMN assigned_engineer SET NOT NULL;
