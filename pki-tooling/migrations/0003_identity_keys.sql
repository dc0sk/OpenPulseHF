BEGIN;

CREATE TABLE IF NOT EXISTS identity_keys (
    revision_id TEXT NOT NULL REFERENCES identity_revisions(revision_id) ON DELETE CASCADE,
    key_id TEXT NOT NULL,
    algorithm TEXT NOT NULL,
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    key_status TEXT NOT NULL CHECK (key_status IN ('active', 'deprecated', 'revoked', 'superseded')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (revision_id, key_id)
);

CREATE INDEX IF NOT EXISTS idx_identity_keys_fingerprint ON identity_keys (fingerprint);
CREATE INDEX IF NOT EXISTS idx_identity_keys_status ON identity_keys (key_status);

COMMIT;
