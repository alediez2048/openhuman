-- F4-9 — approval queue for `ApprovalPolicy::DraftAndApprove` campaigns.
--
-- Outbound externally-visible actions (gmail_send, channel_message,
-- http_request side-effects) under a campaign with DraftAndApprove
-- policy land here instead of firing immediately. The user reviews
-- + edits + approves through `/approvals`; an approval triggers a
-- re-issue path that fires the action with the (possibly edited)
-- payload.
--
-- `status` lifecycle: pending → (approved|rejected) →
-- (sent|failed). `approved` is the user decision; `sent` /
-- `failed` is the post-decision execution outcome (separated so the
-- UI can show "approved, sending..." then update to "sent" or
-- "send failed").

CREATE TABLE IF NOT EXISTS approval_queue (
    id            TEXT PRIMARY KEY,
    campaign_id   TEXT NOT NULL REFERENCES campaigns(id) ON DELETE CASCADE,
    workflow_id   TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    run_id        TEXT NOT NULL,
    node_id       TEXT NOT NULL,
    action_kind   TEXT NOT NULL,
    target        TEXT NOT NULL,
    payload_json  TEXT NOT NULL,
    context_json  TEXT,
    status        TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    decided_at    TEXT,
    decided_by    TEXT,
    error         TEXT
);

CREATE INDEX IF NOT EXISTS idx_approval_queue_status
    ON approval_queue(status);

CREATE INDEX IF NOT EXISTS idx_approval_queue_campaign_status
    ON approval_queue(campaign_id, status);
