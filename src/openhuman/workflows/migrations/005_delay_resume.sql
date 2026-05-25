-- 005_delay_resume.sql — Persistent tracking for delay-node resumes.
--
-- F2-7b adds a `pending_resume_at` column on `workflow_runs` so a run
-- that's pausing inside a `delay` node survives a core restart with
-- enough state to either (a) resume on the next boot if the resume
-- time still falls in the future, or (b) be flipped to a Failed
-- `delay_interrupted` terminal state cleanly when the resume time
-- passed during downtime.
--
-- The column is NULL whenever the run isn't sleeping — `execute_delay`
-- writes the resume timestamp before calling `tokio::sleep` and
-- clears it once the sleep returns. The orphan-recovery sweep at boot
-- distinguishes "crashed mid-LLM-call" rows (NULL) from "validly
-- sleeping" rows (non-NULL) so the latter isn't mistakenly flipped
-- to Failed{CoreCrashed}.
--
-- Full mid-execution resume (re-dispatching the chain from the step
-- AFTER the delay) is a Phase 3 canvas-editor concern that needs
-- richer per-node checkpointing. F2-7b's MVP closes the visibility +
-- correctness gap; the boot path explicitly marks interrupted delays
-- as Failed rather than silently dropping them.

ALTER TABLE workflow_runs ADD COLUMN pending_resume_at TEXT;

-- Index so the boot recovery scan stays O(log n) on a busy workspace.
CREATE INDEX IF NOT EXISTS idx_workflow_runs_pending_resume_at
  ON workflow_runs(pending_resume_at);
