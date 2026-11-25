-- The organiser geodata identifies an engineer by `tabel_id` (AT-0001 …).
-- Registration issues its own uuid, so the account stores the organiser id
-- here and the ranking reads that engineer's licence by it.
ALTER TABLE engineers ADD COLUMN tabel_id VARCHAR NULL;
