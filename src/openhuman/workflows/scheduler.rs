//! Cron-driven trigger dispatch + manual `workflows_run_now` handler.
//!
//! Phase 1 / F-7 ships a small in-process scheduler keyed by an
//! in-memory `HashMap<WorkflowId, Entry>` registry. Each entry holds
//! the workflow's cron expression + the next-run timestamp. A single
//! tokio poll task (started by [`run`]) wakes every
//! [`POLL_INTERVAL_SECS`] seconds, fires any entries whose `next_run`
//! has passed, and advances them to the next cron occurrence.
//!
//! ## Tactical deviation from the F-7 primer
//!
//! The ticket called for reuse of `cron::JobType::WorkflowTrigger`
//! (a new variant on the existing cron domain's JobType enum). That
//! reuse would require:
//!
//!   1. Turning the unit-only `JobType` enum into a struct-variant
//!      enum, breaking the lowercase-string storage in `cron.db`.
//!   2. Adding a `workflow_id` column to `cron_jobs` with a SQL
//!      migration.
//!   3. Updating every existing `JobType::as_str` / `parse` /
//!      dispatch site.
//!
//! A sibling-loop in `workflows::scheduler` reuses the **parsing**
//! API (`cron::normalize_expression` + `cron::Schedule::from_str`)
//! without coupling the storage / dispatch layers. ADR-003 (separate
//! SQLite databases) already endorses domain isolation; this matches
//! that intent. Registration is in-process, so cron jobs survive
//! across cron-domain restarts but the workflow scheduler rebuilds
//! its registry from the `workflows` table via
//! [`reconcile_at_startup`] on every core boot.
//!
//! Trade-offs:
//!   + Zero migration cost; cron.db unchanged.
//!   + Scheduler state is pure derivation of `workflows.enabled +
//!     trigger`; rebuilding from DB is trivial.
//!   - Per-second cron resolution costs a polling loop (acceptable —
//!     polling every 30s for per-minute crons is a 30s worst-case
//!     latency, identical to the cron domain's own poll cadence).
//!
//! F-9 will add single-flight + orphan-recovery on top of the
//! executor; F-7's `dispatch_run` is a stub that simply returns a
//! new run id.

use crate::openhuman::config::Config;
use crate::openhuman::cron::normalize_expression;
use crate::openhuman::workflows::executor;
use crate::openhuman::workflows::store;
use crate::openhuman::workflows::types::{
    ListFilter, ManualInitiator, RunId, RunNowError, Trigger, TriggerSource, Workflow, WorkflowId,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use cron::Schedule as CronSchedule;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Polling interval for the workflow scheduler loop. 30s matches the
/// cron-domain's MIN_POLL_SECONDS upper bound and gives at most a 30s
/// dispatch latency for per-minute crons — well within FR-1.4.1's
/// "best-effort minute-resolution" contract.
const POLL_INTERVAL_SECS: u64 = 30;

#[derive(Debug, Clone)]
struct Entry {
    workflow_id: WorkflowId,
    expr: String,
    next_run: DateTime<Utc>,
}

/// Process-global registry. Cleared + rebuilt on every
/// [`reconcile_at_startup`]; mutated synchronously from
/// [`register`] / [`deregister`].
fn registry() -> &'static Mutex<HashMap<WorkflowId, Entry>> {
    static REGISTRY: OnceLock<Mutex<HashMap<WorkflowId, Entry>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a workflow with the scheduler. No-op when:
///   - the workflow is `enabled = false`, or
///   - the trigger isn't `Trigger::Cron`.
///
/// Returns the next-run timestamp on success (`None` when the
/// workflow was filtered out). Errors only when the cron expression
/// can't be parsed — those should be impossible to reach via the
/// normal create/update flow because F-11's validator catches
/// invalid crons at proposal time; surfacing them here as `Err`
/// keeps the contract honest if a bypass slips through.
pub fn register(workflow: &Workflow) -> Result<Option<DateTime<Utc>>> {
    if !workflow.enabled {
        return Ok(None);
    }
    let Trigger::Cron { expr, .. } = &workflow.trigger else {
        return Ok(None);
    };
    let normalized = normalize_expression(expr)?;
    let schedule = CronSchedule::from_str(&normalized)
        .map_err(|e| anyhow::anyhow!("invalid cron `{expr}`: {e}"))?;
    let now = Utc::now();
    let next_run = schedule
        .after(&now)
        .next()
        .ok_or_else(|| anyhow::anyhow!("cron `{expr}` produced no future occurrence"))?;
    let entry = Entry {
        workflow_id: workflow.id.clone(),
        expr: expr.clone(),
        next_run,
    };
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.insert(workflow.id.clone(), entry);
    tracing::info!(
        target: "workflows-scheduler",
        "[workflows-scheduler] register wf={} cron='{expr}' next_run={next_run}",
        workflow.id
    );
    Ok(Some(next_run))
}

/// Remove a workflow from the scheduler registry. Idempotent — no-op
/// if the workflow wasn't registered.
pub fn deregister(workflow_id: &WorkflowId) {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    if reg.remove(workflow_id).is_some() {
        tracing::info!(
            target: "workflows-scheduler",
            "[workflows-scheduler] deregister wf={workflow_id}"
        );
    }
}

/// Rebuild the registry from the `workflows` table. Called on core
/// boot (per FR-1.4.1.1, restart shouldn't drop scheduled jobs) and
/// safe to call again — any prior entries are cleared first so the
/// post-reconcile state is exactly the current persisted state.
pub async fn reconcile_at_startup(config: &Config) -> Result<usize> {
    {
        let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
        reg.clear();
    }
    let workflows = store::list_workflows(
        config,
        &ListFilter {
            enabled: Some(true),
            ..Default::default()
        },
    )?;
    let mut count = 0;
    for wf in workflows {
        if !wf.trigger.is_cron() {
            continue;
        }
        match register(&wf) {
            Ok(Some(_)) => count += 1,
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    target: "workflows-scheduler",
                    "[workflows-scheduler] reconcile: skipping wf={} bad-cron err={err:#}",
                    wf.id
                );
            }
        }
    }
    tracing::info!(
        target: "workflows-scheduler",
        "[workflows-scheduler] reconcile_at_startup count={count}"
    );
    Ok(count)
}

/// Polling loop. Wakes every [`POLL_INTERVAL_SECS`] seconds, fires
/// any registered workflows whose `next_run` has passed, and advances
/// the entry to its next cron occurrence.
///
/// Run on a long-lived tokio task spawned at core boot (after
/// [`reconcile_at_startup`]). The loop never exits — shutdown is
/// owned by the task's abort handle, which the host owns.
pub async fn run(config: Config) -> Result<()> {
    tracing::info!(
        target: "workflows-scheduler",
        "[workflows-scheduler] poll loop started interval={POLL_INTERVAL_SECS}s"
    );
    let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
    // Skip the first immediate tick; otherwise the loop fires once at
    // boot before any cron is actually due.
    interval.tick().await;
    loop {
        interval.tick().await;
        let due = drain_due_entries(Utc::now());
        if due.is_empty() {
            continue;
        }
        tracing::debug!(
            target: "workflows-scheduler",
            "[workflows-scheduler] firing {} due workflow(s)",
            due.len()
        );
        for entry in due {
            let config = config.clone();
            tokio::spawn(async move {
                let workflow_id = entry.workflow_id.clone();
                match executor::dispatch_run(&config, workflow_id.clone(), TriggerSource::Cron)
                    .await
                {
                    Ok(run_id) => {
                        tracing::info!(
                            target: "workflows-scheduler",
                            "[workflows-scheduler] cron tick fired wf={workflow_id} run={run_id}"
                        );
                    }
                    Err(err) => {
                        tracing::error!(
                            target: "workflows-scheduler",
                            "[workflows-scheduler] cron tick dispatch failed wf={workflow_id}: {err:#}"
                        );
                    }
                }
            });
        }
    }
}

/// Return every entry whose `next_run` has passed, advancing each
/// entry to its next cron occurrence in the registry. Held the lock
/// for the whole pass — the operations inside are O(N) over the
/// registered set and don't await.
fn drain_due_entries(now: DateTime<Utc>) -> Vec<Entry> {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    let mut fired = Vec::new();
    for (id, entry) in reg.iter_mut() {
        if entry.next_run > now {
            continue;
        }
        fired.push(Entry {
            workflow_id: id.clone(),
            expr: entry.expr.clone(),
            next_run: entry.next_run,
        });
        // Advance to the next future occurrence. If parsing somehow
        // fails (shouldn't — register() already validated), keep the
        // entry but set next_run to + POLL_INTERVAL so the registry
        // doesn't busy-loop on the same expression.
        match normalize_expression(&entry.expr).and_then(|n| {
            CronSchedule::from_str(&n).map_err(|e| anyhow::anyhow!("re-parse failed: {e}"))
        }) {
            Ok(schedule) => match schedule.after(&now).next() {
                Some(next) => entry.next_run = next,
                None => entry.next_run = now + chrono::Duration::seconds(3600),
            },
            Err(err) => {
                tracing::error!(
                    target: "workflows-scheduler",
                    "[workflows-scheduler] failed to re-parse cron for wf={id}: {err:#}; backing off 60s"
                );
                entry.next_run = now + chrono::Duration::seconds(60);
            }
        }
    }
    fired
}

/// Drives the `workflows_run_now` RPC. Per FR-1.4.1.2 + FR-1.4.1.3:
///   - the gate is `health == Ready`, NOT `enabled` (manual runs can
///     fire disabled workflows; the enabled bit governs cron only),
///   - missing workflow id returns [`RunNowError::NotFound`].
pub async fn handle_run_now(
    config: &Config,
    workflow_id: WorkflowId,
    initiator: ManualInitiator,
) -> Result<RunId, RunNowError> {
    let workflow = match store::get_workflow(config, &workflow_id) {
        Ok(Some(w)) => w,
        Ok(None) => return Err(RunNowError::NotFound),
        Err(err) => {
            return Err(RunNowError::Dispatch {
                reason: format!("store::get_workflow failed: {err:#}"),
            });
        }
    };
    if !matches!(
        workflow.health,
        crate::openhuman::workflows::types::WorkflowHealth::Ready
    ) {
        return Err(RunNowError::HealthBlocked {
            health: workflow.health,
        });
    }
    let source = TriggerSource::Manual {
        initiator: initiator.label(),
    };
    executor::dispatch_run(config, workflow_id, source)
        .await
        .map_err(|err| RunNowError::Dispatch {
            reason: format!("{err:#}"),
        })
}

// ── Test-only helpers ──────────────────────────────────────────────────

/// Reset the registry. Tests call this before driving register/
/// deregister to isolate from other tests that share the singleton.
#[cfg(test)]
pub(crate) fn reset_registry_for_test() {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.clear();
}

/// Snapshot the registered workflow ids. Tests assert against this.
#[cfg(test)]
pub(crate) fn registered_ids_for_test() -> Vec<WorkflowId> {
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    let mut ids: Vec<_> = reg.keys().cloned().collect();
    ids.sort();
    ids
}
