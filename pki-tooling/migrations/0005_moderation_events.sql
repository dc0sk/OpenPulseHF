BEGIN;

CREATE TABLE IF NOT EXISTS moderation_events (
    event_id TEXT PRIMARY KEY,
    submission_id TEXT NOT NULL REFERENCES submissions(submission_id) ON DELETE CASCADE,
    actor_identity TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('accept', 'reject', 'quarantine', 'reopen')),
    reason_code TEXT NOT NULL,
    reason_text TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_moderation_events_submission_id ON moderation_events (submission_id);
CREATE INDEX IF NOT EXISTS idx_moderation_events_created_at ON moderation_events (created_at);

COMMIT;