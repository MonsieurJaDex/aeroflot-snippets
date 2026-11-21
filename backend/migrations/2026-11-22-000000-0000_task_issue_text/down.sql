ALTER TABLE tasks
    ALTER COLUMN issue_type TYPE aircraft_issue
    USING (
        CASE
            WHEN issue_type = ANY (enum_range(NULL::aircraft_issue)::text[])
                THEN issue_type::aircraft_issue
            ELSE 'other'::aircraft_issue
        END
    );
