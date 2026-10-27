-- Program profiles: what each program is about, read from the MySchools
-- school detail documents and classified by keyword rules. Versioned like
-- the other reference sets.

CREATE TABLE profile_snapshots (
    id BIGSERIAL PRIMARY KEY,
    recorded_at TIMESTAMPTZ NOT NULL,
    source TEXT NOT NULL
);

CREATE TABLE program_profiles (
    snapshot_id BIGINT NOT NULL REFERENCES profile_snapshots(id) ON DELETE CASCADE,
    code TEXT NOT NULL CHECK (code <> ''),
    dbn TEXT NOT NULL,
    program_name TEXT NOT NULL,
    school_name TEXT NOT NULL,
    method TEXT NOT NULL,
    description TEXT NOT NULL,
    overview TEXT NOT NULL,
    academic_opportunities TEXT NOT NULL,
    art_courses TEXT NOT NULL,
    language_classes TEXT NOT NULL,
    -- [{focus, confidence}]: every kind with evidence and its confidence,
    -- 0 to 1. The application tags a program with the kinds at or above
    -- its threshold.
    scores JSONB NOT NULL,
    -- [{focus, keyword, label, weight}]: each keyword match, the text it
    -- was found in, and the weight of that text.
    evidence JSONB NOT NULL,
    PRIMARY KEY (snapshot_id, code)
);
