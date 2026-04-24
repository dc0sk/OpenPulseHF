-- Request tracking for idempotent write operations
CREATE TABLE request_tracking (
    request_id TEXT PRIMARY KEY,
    endpoint TEXT NOT NULL,
    method TEXT NOT NULL,
    response_status INT NOT NULL,
    response_body_hash TEXT NOT NULL,
    response_body_json JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE INDEX idx_request_tracking_expires_at ON request_tracking (expires_at);
CREATE INDEX idx_request_tracking_created_at ON request_tracking (created_at);
