-- 008_campaigns.sql — Phase 4 F4-2: campaigns table + workflows.campaign_id FK.
--
-- Lands the persistence layer for `Campaign` (F4-1 types) alongside the
-- existing workflows tables in `workflows.db` (per ADR-003, campaigns +
-- workflows share a DB because cross-domain queries — "every workflow
-- under campaign X" — would otherwise require an attach/join across
-- separate SQLite files).
--
-- ## Schema
--
-- `campaigns` row carries the lifecycle status, entity binding, throttle,
-- approval policy, and target outcome as JSON-blob columns matching the
-- workflows-table convention. The `Campaign` struct's structured fields
-- round-trip as JSON in their dedicated TEXT columns.
--
-- `workflows.campaign_id` is a NULLABLE FK with `ON DELETE SET NULL` —
-- not cascade. A campaign delete is a user mistake risk; we orphan
-- workflows rather than lose them. The F2-14 retention sweep can later
-- purge archived campaigns + orphaned workflows separately.
--
-- ## Soft-delete
--
-- Mirrors the F2-14 pattern on workflows: `deleted_at` column, NULL on
-- live rows, set to ISO-8601 timestamp on delete. `list_campaigns` excludes
-- deleted rows by default; the retention sweep eventually hard-deletes
-- rows whose `deleted_at` is older than 30 days.

CREATE TABLE IF NOT EXISTS campaigns (
    id              TEXT PRIMARY KEY,
    schema_version  INTEGER NOT NULL DEFAULT 1,
    name            TEXT NOT NULL,
    description     TEXT,
    status          TEXT NOT NULL,
    entity_binding  TEXT NOT NULL,
    throttle        TEXT,
    approval_policy TEXT NOT NULL,
    target_outcome  TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    last_run_at     TEXT,
    deleted_at      TEXT
);

CREATE INDEX IF NOT EXISTS idx_campaigns_status      ON campaigns(status);
CREATE INDEX IF NOT EXISTS idx_campaigns_updated_at  ON campaigns(updated_at);
CREATE INDEX IF NOT EXISTS idx_campaigns_deleted_at  ON campaigns(deleted_at);

ALTER TABLE workflows ADD COLUMN campaign_id TEXT
    REFERENCES campaigns(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_workflows_campaign_id ON workflows(campaign_id);
