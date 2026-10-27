-- Reference data, versioned: every refresh or import adds a snapshot and
-- the server reads the latest one. Older snapshots stay for the record.

CREATE TABLE catalog_snapshots (
    id BIGSERIAL PRIMARY KEY,
    recorded_at TIMESTAMPTZ NOT NULL,
    source TEXT NOT NULL,
    school_count INTEGER NOT NULL CHECK (school_count >= 0)
);

-- One MySchools school document per row, in download order.
CREATE TABLE catalog_schools (
    snapshot_id BIGINT NOT NULL REFERENCES catalog_snapshots(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    document JSONB NOT NULL,
    PRIMARY KEY (snapshot_id, position)
);

CREATE TABLE score_snapshots (
    id BIGSERIAL PRIMARY KEY,
    recorded_at TIMESTAMPTZ NOT NULL,
    source TEXT NOT NULL
);

CREATE TABLE school_scores (
    snapshot_id BIGINT NOT NULL REFERENCES score_snapshots(id) ON DELETE CASCADE,
    dbn TEXT NOT NULL CHECK (dbn <> ''),
    year INTEGER NOT NULL,
    ela_proficient_pct DOUBLE PRECISION,
    math_proficient_pct DOUBLE PRECISION,
    PRIMARY KEY (snapshot_id, dbn)
);

CREATE TABLE program_sets (
    id BIGSERIAL PRIMARY KEY,
    recorded_at TIMESTAMPTZ NOT NULL,
    source TEXT NOT NULL
);

-- One hand-entered program per row, in the programs.json document shape.
CREATE TABLE programs (
    set_id BIGINT NOT NULL REFERENCES program_sets(id) ON DELETE CASCADE,
    code TEXT NOT NULL CHECK (code <> ''),
    document JSONB NOT NULL,
    PRIMARY KEY (set_id, code)
);

CREATE TABLE student_defaults (
    id BIGSERIAL PRIMARY KEY,
    recorded_at TIMESTAMPTZ NOT NULL,
    source TEXT NOT NULL,
    document JSONB NOT NULL
);

-- The primary keys serve every query: "latest snapshot" reads ORDER BY id
-- DESC LIMIT 1 on the snapshot tables, and the detail tables are read by
-- snapshot id, the leading column of their primary key.
