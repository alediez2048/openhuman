-- 001_init_workflows.sql — Workflow definitions.
--
-- Owns the catalog of workflows the user has saved. Runs + run-steps
-- arrive in 002 and 003. The `*_json` columns hold JSON-encoded
-- structured values (Trigger, Vec<Node>, Vec<Edge>, WorkflowSettings,
-- WorkflowOrigin, WorkflowHealth) — Phase 1 stores blobs rather than
-- normalising sub-rows. See ADR-003 + ADR-017.

CREATE TABLE IF NOT EXISTS workflows (
  id              TEXT    PRIMARY KEY,
  schema_version  INTEGER NOT NULL DEFAULT 1,
  name            TEXT    NOT NULL,
  description     TEXT,
  enabled         INTEGER NOT NULL DEFAULT 0,    -- bool
  origin          TEXT    NOT NULL,              -- JSON-encoded WorkflowOrigin
  health          TEXT    NOT NULL,              -- JSON-encoded WorkflowHealth
  trigger_json    TEXT    NOT NULL,
  nodes_json      TEXT    NOT NULL,
  edges_json      TEXT    NOT NULL,
  settings_json   TEXT    NOT NULL,
  created_at      TEXT    NOT NULL,
  updated_at      TEXT    NOT NULL,
  last_run_at     TEXT
);

-- List-view filter chips key off these columns.
CREATE INDEX IF NOT EXISTS idx_workflows_enabled     ON workflows(enabled);
CREATE INDEX IF NOT EXISTS idx_workflows_updated_at  ON workflows(updated_at);
CREATE INDEX IF NOT EXISTS idx_workflows_last_run_at ON workflows(last_run_at);
-- Indexed string prefix is enough for SQLite's LIKE on the JSON-encoded
-- discriminator (e.g. `"needs_connections"`). Real predicate scans land
-- with the F-3 subscriber's `list_workflows_referencing` query.
CREATE INDEX IF NOT EXISTS idx_workflows_health      ON workflows(health);

-- Migration ledger — used by store::apply_migrations.
CREATE TABLE IF NOT EXISTS schema_migrations (
  version    INTEGER PRIMARY KEY,
  applied_at TEXT    NOT NULL
);
