BEGIN;

CREATE TABLE IF NOT EXISTS identity_records (
    record_id TEXT PRIMARY KEY,
    station_id TEXT NOT NULL,
    callsign TEXT NOT NULL,
    current_revision_id TEXT,
    publication_state TEXT NOT NULL CHECK (publication_state IN ('pending', 'published', 'quarantined', 'rejected', 'revoked')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_identity_records_station_id ON identity_records (station_id);
CREATE INDEX IF NOT EXISTS idx_identity_records_publication_state ON identity_records (publication_state);

COMMIT;
