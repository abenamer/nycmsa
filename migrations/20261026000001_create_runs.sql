-- Simulation runs. The student and report are stored as the JSON documents
-- the HTTP API also serves; the application validates them again on read.
CREATE TABLE runs (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL,
    student JSONB NOT NULL,
    report JSONB NOT NULL
);
-- The primary key serves both queries: `find` by id and `recent`
-- (ORDER BY id DESC LIMIT n). No other index is needed for this workload.
