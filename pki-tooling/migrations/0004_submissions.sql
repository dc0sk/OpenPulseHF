BEGIN;

CREATE TABLE IF NOT EXISTS submissions (
    submission_id TEXT PRIMARY KEY,
    submitter_identity TEXT NOT NULL,
    submission_state TEXT NOT NULL CHECK (submission_state IN ('pending', 'accepted', 'quarantined', 'rejected')),
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    artifact_uri TEXT NOT NULL,
    detached_signature_uri TEXT,
    validation_summary JSONB NOT NULL,
    moderation_reason_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_submissions_state_received_at ON submissions (submission_state, received_at);

COMMIT;
