-- 001_init_generic_http.sql — Phase 0 (P0-1)
-- Owned by src/openhuman/connections/. Writes to ${OPENHUMAN_WORKSPACE}/connections.db.

CREATE TABLE IF NOT EXISTS generic_http_connections (
  id              TEXT PRIMARY KEY,
  name            TEXT NOT NULL,
  base_url        TEXT NOT NULL,
  auth_kind       TEXT NOT NULL,       -- JSON-encoded AuthKind
  secret_ref      TEXT,                -- nullable; JSON-encoded SecretRef or NULL
  default_headers TEXT NOT NULL,       -- JSON-encoded Vec<(String,String)>
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_generic_http_updated_at
  ON generic_http_connections(updated_at);

CREATE TABLE IF NOT EXISTS schema_migrations (
  version    INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);
