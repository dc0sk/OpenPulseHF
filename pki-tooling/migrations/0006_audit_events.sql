BEGIN;

CREATE TABLE IF NOT EXISTS audit_events (
    event_id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    actor_identity TEXT NOT NULL,
    request_id TEXT,
    event_payload_hash TEXT NOT NULL,
    event_payload_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_events_entity ON audit_events (entity_type, entity_id, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_events_event_type ON audit_events (event_type, created_at);

COMMIT;