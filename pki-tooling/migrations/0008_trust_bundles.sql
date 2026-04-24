BEGIN;

CREATE TABLE IF NOT EXISTS trust_bundles (
    bundle_id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    generated_at TIMESTAMPTZ NOT NULL,
    issuer_instance_id TEXT NOT NULL,
    signing_algorithms JSONB NOT NULL,
    records JSONB NOT NULL,
    bundle_signature TEXT NOT NULL,
    is_current BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_trust_bundles_current
    ON trust_bundles (is_current)
    WHERE is_current = TRUE;
CREATE INDEX IF NOT EXISTS idx_trust_bundles_generated_at ON trust_bundles (generated_at DESC);

COMMIT;
