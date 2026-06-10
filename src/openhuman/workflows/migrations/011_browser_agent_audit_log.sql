-- 011_browser_agent_audit_log.sql — F3-6 chunk 2 per-tool-call audit log.
--
-- Every browser_observe / browser_act / browser_extract call inside a
-- BrowserAction workflow node writes one row here. Independent of
-- `workflow_run_steps` because a single node can fire 20+ tool calls
-- and cramming them into `run_steps.output_json` would break the
-- run-detail UI's structured rendering.
--
-- Read path: `browser_agent_get_audit_log(run_id)` RPC for the run-
-- detail UI (consumer lands when F3-5's preview surface ships).
-- Retention: hard-deleted after N days by the workflow retention
-- sweep (default 30; configurable in a future ticket).

CREATE TABLE browser_agent_audit_log (
    id              TEXT PRIMARY KEY,
    run_id          TEXT NOT NULL,
    step_id         TEXT,
    -- ISO-8601 (UTC). Indexed below for time-range queries.
    timestamp       TEXT NOT NULL,
    tool_name       TEXT NOT NULL,
    -- JSON-encoded subset of the tool args. Sensitive fields are
    -- stripped at write-time per `browser_agent::safety::redaction`
    -- (F3-6 chunk 3 — redaction lands with screenshots, until then
    -- the writer just persists the args as-is).
    args_json       TEXT NOT NULL,
    -- Human-readable one-liner summarising the tool's outcome.
    result_summary  TEXT NOT NULL,
    -- F3-6 chunk 3: filesystem path under
    -- `{workspace}/browser_audit/<run_id>/...` when a screenshot was
    -- captured. NULL until screenshots ship.
    screenshot_path TEXT,
    -- F3-6 chunk 3: count of fields the redaction pass scrubbed.
    -- Surfaces in the run-detail UI so the user knows redaction ran.
    -- Default 0 until redaction ships.
    redacted_fields_count INTEGER NOT NULL DEFAULT 0
);

-- Listing path: "show all browser-agent calls for this run, in order".
CREATE INDEX idx_browser_audit_run_id ON browser_agent_audit_log(run_id, timestamp);
