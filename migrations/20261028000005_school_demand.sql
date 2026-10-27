-- School demand history: the DOE's reports on school applications, offers
-- and seats (Local Law 72 of 2018, formerly 1486-A), grade 6 only, per
-- school and admissions year, overall and by the applicants' residential
-- district. Versioned like the other reference sets: every refresh adds a
-- snapshot and the server reads the latest one.

CREATE TABLE demand_snapshots (
    id BIGSERIAL PRIMARY KEY,
    recorded_at TIMESTAMPTZ NOT NULL,
    source TEXT NOT NULL
);

CREATE TABLE school_demand (
    snapshot_id BIGINT NOT NULL REFERENCES demand_snapshots(id) ON DELETE CASCADE,
    dbn TEXT NOT NULL CHECK (dbn <> ''),
    -- The fall of entry: 2025 for offers made in the spring of 2025.
    year INTEGER NOT NULL,
    -- '' for all applicants; otherwise the residential district code
    -- without leading zeros, such as '15'.
    district TEXT NOT NULL,
    -- NULL is a figure the DOE suppressed or did not publish that year.
    seats INTEGER CHECK (seats >= 0),
    listed INTEGER CHECK (listed >= 0),
    considered INTEGER CHECK (considered >= 0),
    offers INTEGER CHECK (offers >= 0),
    PRIMARY KEY (snapshot_id, dbn, year, district)
);
