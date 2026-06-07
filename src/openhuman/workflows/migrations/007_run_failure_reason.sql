-- 007_run_failure_reason.sql — Trust UX T-4 structured failure
-- classification on workflow runs.
--
-- T-4 introduces `FailureReason` — a stable enum classifying *why* a
-- workflow run failed (`agent_narrated_without_acting`,
-- `composio_upstream_rejected`, `model_unavailable`, `llm_auth_failed`,
-- `connection_expired`, `tool_slug_invalid`, `unknown`). Populated by
-- `failure_classifier::classify_failure` at the moment the executor
-- flips a step to RunStatus::Failed; persisted here so the per-run
-- outcome card (T-2) can render a curated one-liner + fix-it action
-- instead of forcing the user to parse the raw `error` string.
--
-- Stored as a JSON-encoded `FailureReason` (enum-with-payload). NULL
-- when status != Failed and for pre-T-4 runs — the deserialiser
-- treats absent / NULL as None, so old rows continue to work
-- unchanged.

ALTER TABLE workflow_runs
  ADD COLUMN failure_reason_json TEXT;
