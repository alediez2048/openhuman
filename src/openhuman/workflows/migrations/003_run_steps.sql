-- 003_run_steps.sql — Per-node step rows inside a run.
--
-- One row per node-execution within a run. `output_json` is capped at
-- 64 KiB by the executor (NFR-2.3.5). Deleting a run cascades all of
-- its steps.

CREATE TABLE IF NOT EXISTS workflow_run_steps (
  id            TEXT    PRIMARY KEY,
  run_id        TEXT    NOT NULL,
  node_id       TEXT    NOT NULL,
  status        TEXT    NOT NULL,                -- RunStatus, snake_case
  started_at    TEXT    NOT NULL,
  completed_at  TEXT,
  output_json   TEXT,                            -- ≤ 64 KiB; UTF-8 boundary safe
  error         TEXT,
  FOREIGN KEY (run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_workflow_run_steps_run_id ON workflow_run_steps(run_id);
