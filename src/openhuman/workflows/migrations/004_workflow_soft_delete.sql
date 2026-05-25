-- 004_workflow_soft_delete.sql — Soft-delete column for retention.
--
-- F2-14 converts the Phase 1 hard-delete into a soft-delete + 30-day
-- retention sweep. New column `deleted_at` defaults to NULL (live row);
-- `ops::delete` sets it to NOW(); the background sweep
-- (`retention::run_purge_sweep`) hard-deletes rows whose `deleted_at`
-- is older than the retention window (30 days per FR-1.3.4).
--
-- Run history (`workflow_runs` + `workflow_run_steps`) survives the
-- soft-delete window so users restoring a workflow can see what fired
-- before deletion. The eventual hard-purge cascades via the existing
-- FK chain from 002 / 003.

ALTER TABLE workflows ADD COLUMN deleted_at TEXT;

-- Index so the background sweep + list-view filter stay O(log n).
CREATE INDEX IF NOT EXISTS idx_workflows_deleted_at ON workflows(deleted_at);
