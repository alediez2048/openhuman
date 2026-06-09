-- F4-8 — campaign throttle state.
--
-- A campaign's `Throttle { max_per_window, window }` is enforced
-- across all its sub-workflows and `for_each` iterations. The gate
-- buckets reservations by the window's epoch boundary (midnight UTC
-- for PerDay, top of the hour for PerHour, top of the minute for
-- PerMinute) and persists the running count so a core restart can
-- pick up the same budget without double-spending.
--
-- The composite PK lets concurrent reservations land deterministic
-- INSERT-OR-UPDATEs: ON CONFLICT bumps `consumed` instead of forking
-- two rows for the same bucket. Older window rows can be culled by
-- a periodic sweep; the gate itself only reads / writes the current
-- bucket.

CREATE TABLE IF NOT EXISTS campaign_throttle_state (
    campaign_id   TEXT NOT NULL,
    window_start  TEXT NOT NULL,
    consumed      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (campaign_id, window_start)
);

CREATE INDEX IF NOT EXISTS idx_campaign_throttle_campaign
    ON campaign_throttle_state(campaign_id);
