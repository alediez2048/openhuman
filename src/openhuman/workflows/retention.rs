//! F2-14 — soft-delete retention sweep.
//!
//! Workflow soft-delete (`ops::delete` → `store::delete_workflow`) sets
//! `deleted_at` to NOW and leaves `workflow_runs` + `workflow_run_steps`
//! in place. The user has a 30-day window
//! ([`DEFAULT_RETENTION_DAYS`]) to restore via `workflows_restore`.
//! Past that window this sweep hard-deletes the row + cascades the FK
//! chain.
//!
//! ## Cadence
//!
//! `run_purge_sweep` is intended to fire once per hour from a tokio
//! task spawned at boot. The work per tick is bounded:
//! `list_deleted_workflows_older_than` is indexed (`idx_workflows_deleted_at`)
//! and the candidate set is small in practice.
//!
//! ## Testing
//!
//! The sweep takes a `now_provider: impl Fn() -> DateTime<Utc>` so tests
//! can fast-forward past the retention window without sleeping. Production
//! passes `Utc::now`. The `now_provider` is captured once per call —
//! callers needing per-iteration freshness re-invoke
//! `run_purge_sweep_with_now` themselves.

use crate::core::event_bus::{publish_global, DomainEvent};
use crate::openhuman::config::Config;
use crate::openhuman::workflows::store;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};

/// FR-1.3.4 — 30-day soft-delete retention window. Surfaced in the
/// `WorkflowDeletePreview.retention_days` field so the UI and this
/// sweep stay in sync.
pub const DEFAULT_RETENTION_DAYS: i64 = 30;

/// Sweep cadence. Hourly per the F2-14 architectural decision (one
/// purge/h is cheap; daily creates a thundering-herd window).
pub const SWEEP_INTERVAL_SECS: u64 = 60 * 60;

/// Run the retention sweep with `Utc::now` as the clock. Production
/// boot path calls this on each tokio-interval tick.
pub fn run_purge_sweep(config: &Config) -> Result<u32> {
    run_purge_sweep_with_now(config, Utc::now, DEFAULT_RETENTION_DAYS)
}

/// Test-friendly variant: caller injects the `now` clock + retention
/// window. Returns the number of rows that were hard-deleted in this
/// tick (0 when no rows aged out).
pub fn run_purge_sweep_with_now(
    config: &Config,
    now_provider: impl Fn() -> DateTime<Utc>,
    retention_days: i64,
) -> Result<u32> {
    let now = now_provider();
    let cutoff = now - Duration::days(retention_days);
    let aged = store::list_deleted_workflows_older_than(config, cutoff)?;
    if aged.is_empty() {
        tracing::trace!(
            target: "workflows-retention",
            "[workflows-retention] sweep tick — no rows aged out (cutoff={cutoff})"
        );
        return Ok(0);
    }

    let mut purged = 0u32;
    for (id, deleted_at) in &aged {
        match store::hard_delete_workflow(config, id) {
            Ok(run_count) => {
                purged += 1;
                tracing::info!(
                    target: "workflows-retention",
                    "[workflows-retention] purged wf={id} deleted_at={deleted_at} runs_dropped={run_count}"
                );
                publish_global(DomainEvent::WorkflowPurged {
                    workflow_id: id.clone(),
                    run_count,
                });
            }
            Err(err) => {
                tracing::error!(
                    target: "workflows-retention",
                    "[workflows-retention] hard_delete_workflow failed wf={id}: {err:#}"
                );
            }
        }
    }

    tracing::info!(
        target: "workflows-retention",
        "[workflows-retention] sweep tick complete — purged {purged}/{} candidates",
        aged.len()
    );
    Ok(purged)
}
