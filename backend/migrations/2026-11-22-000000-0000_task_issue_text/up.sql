ALTER TABLE tasks
    ALTER COLUMN issue_type TYPE TEXT USING issue_type::text;
