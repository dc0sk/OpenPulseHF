BEGIN;

CREATE TABLE IF NOT EXISTS revocations (
    revocation_id TEXT PRIMARY KEY,
    record_id TEXT NOT NULL REFERENCES identity_records(record_id) ON DELETE CASCADE,
    revision_id TEXT,
    key_id TEXT,
    issuer_identity TEXT NOT NULL,
    reason_code TEXT NOT NULL,
    effective_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_revocations_record_id_effective_at ON revocations (record_id, effective_at);
CREATE INDEX IF NOT EXISTS idx_revocations_issuer_identity ON revocations (issuer_identity);
CREATE INDEX IF NOT EXISTS idx_revocations_key_id ON revocations (key_id);

COMMIT;
