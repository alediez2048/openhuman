-- 002_runs.sql — Per-run history rows.
--
-- One row per dispatched execution. Deleting a workflow cascades the
-- run history via the FK below. `cancelled` is ADR-014's soft-cancel
-- flag — `status` may still be `Running` while `cancelled = 1` until
-- the executor observes the cancel and writes the terminal status.

CREATE TABLE IF NOT EXISTS workflow_runs (
  id              TEXT    PRIMARY KEY,
  workflow_id     TEXT    NOT NULL,
  trigger_source  TEXT    NOT NULL,              -- JSON-encoded TriggerSource
  status          TEXT    NOT NULL,              -- RunStatus, snake_case
  started_at      TEXT    NOT NULL,
  completed_at    TEXT,
  error           TEXT,
  cancelled       INTEGER NOT NULL DEFAULT 0,    -- bool (ADR-014)
  FOREIGN KEY (workflow_id) REFERENCES workflows(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_workflow_runs_workflow_id ON workflow_runs(workflow_id);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_status      ON workflow_runs(status);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_started_at  ON workflow_runs(started_at);
