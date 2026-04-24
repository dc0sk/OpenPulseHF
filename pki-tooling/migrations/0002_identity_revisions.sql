BEGIN;

CREATE TABLE IF NOT EXISTS identity_revisions (
    revision_id TEXT PRIMARY KEY,
    record_id TEXT NOT NULL REFERENCES identity_records(record_id) ON DELETE CASCADE,
    revision_number INTEGER NOT NULL,
    valid_from TIMESTAMPTZ NOT NULL,
    valid_until TIMESTAMPTZ,
    submitted_via TEXT NOT NULL CHECK (submitted_via IN ('api', 'web', 'replication')),
    submission_id TEXT,
    algorithms_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (record_id, revision_number)
);

CREATE INDEX IF NOT EXISTS idx_identity_revisions_record_id ON identity_revisions (record_id);

COMMIT;
