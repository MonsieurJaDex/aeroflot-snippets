CREATE TABLE aircraft_movements (
    id UUID PRIMARY KEY,
    aircraft_id UUID NOT NULL,
    map_id TEXT NOT NULL,
    plane_x BIGINT NOT NULL,
    plane_y BIGINT NOT NULL,
    arrived_at TIMESTAMPTZ NOT NULL,
    departed_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX aircraft_movements_one_open
    ON aircraft_movements (map_id, aircraft_id)
    WHERE departed_at IS NULL;
