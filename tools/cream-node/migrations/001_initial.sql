CREATE TABLE contracts (
    contract_instance_id  TEXT PRIMARY KEY,
    contract_key_bytes    BYTEA NOT NULL,
    contract_type         TEXT NOT NULL,
    parameters_bytes      BYTEA NOT NULL DEFAULT '',
    state_bytes           BYTEA NOT NULL,
    state_json            JSONB NOT NULL DEFAULT '{}',
    wasm_code             BYTEA,
    code_hash             BYTEA NOT NULL,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE audit_log (
    id            BIGSERIAL PRIMARY KEY,
    ts            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    entity_type   TEXT NOT NULL,
    entity_id     TEXT NOT NULL,
    action        TEXT NOT NULL,
    old_state     JSONB,
    new_state     JSONB,
    update_data   JSONB,
    metadata      JSONB NOT NULL DEFAULT '{}'
);

CREATE INDEX idx_audit_entity ON audit_log (entity_type, entity_id, ts);
CREATE INDEX idx_audit_ts ON audit_log (ts);

-- Immutability enforcement
CREATE RULE audit_no_update AS ON UPDATE TO audit_log DO INSTEAD NOTHING;
CREATE RULE audit_no_delete AS ON DELETE TO audit_log DO INSTEAD NOTHING;
