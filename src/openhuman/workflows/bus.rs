//! Event-bus subscribers for the Workflows domain.
//!
//! Phase 1 / F-3 ships [`WorkflowHealthRecomputeSubscriber`]: listens
//! for connection-domain events
//! (`ConnectionAdded` / `ConnectionRemoved` / `ConnectionUpdated`),
//! recomputes `WorkflowHealth` for every affected workflow with a
//! bounded SQL UPDATE per event, and publishes
//! `WorkflowHealthChanged` on every REAL transition.
//!
//! Bounded-work contract (per `Automations/systemsdesign.md §8.1`):
//! one `UPDATE` per affected workflow per event, never an unbounded
//! full-table scan. The `store::list_workflows_referencing` LIKE
//! pre-filter narrows the candidate set; the recompute pass filters
//! again through `health::referenced_connections` so the lossy LIKE
//! over-selects cheaply rather than under-selecting.

use crate::core::event_bus::{publish_global, subscribe_global, DomainEvent, EventHandler};
use crate::openhuman::config::Config;
use crate::openhuman::connections::aggregator;
use crate::openhuman::connections::types::ConnectionRef;
use crate::openhuman::workflows::executor;
use crate::openhuman::workflows::health::{self, ConnectionsSnapshot};
use crate::openhuman::workflows::store;
use crate::openhuman::workflows::types::TriggerSource;
use async_trait::async_trait;
use std::sync::Arc;

/// Boot-time registration helper. Subscribes
/// [`WorkflowHealthRecomputeSubscriber`] to the global bus and leaks
/// the handle so the background task lives for the lifetime of the
/// process. Logs a warning if the bus isn't initialised yet (mirrors
/// the pattern in `health::bus::register_health_subscriber` and
/// `composio::register_composio_trigger_subscriber`).
pub fn register_health_recompute_subscriber(config: Arc<Config>) {
    let subscriber = Arc::new(WorkflowHealthRecomputeSubscriber::new(config));
    match subscribe_global(subscriber) {
        Some(handle) => {
            tracing::info!(
                target: "workflows-bus",
                "[workflows-bus] registered health-recompute subscriber"
            );
            std::mem::forget(handle);
        }
        None => {
            log::warn!(
                "[event_bus] failed to register workflows health-recompute subscriber — bus not initialized"
            );
        }
    }
}

/// Subscriber that keeps `workflows.health` in sync with the
/// connections snapshot. Constructed at core boot with a shared
/// `Arc<Config>` so each event handler can open ephemeral SQLite
/// connections without holding state itself.
pub struct WorkflowHealthRecomputeSubscriber {
    config: Arc<Config>,
}

impl WorkflowHealthRecomputeSubscriber {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl EventHandler for WorkflowHealthRecomputeSubscriber {
    fn name(&self) -> &str {
        "workflow::health_recompute"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["connection"])
    }

    async fn handle(&self, event: &DomainEvent) {
        let r#ref = match event {
            DomainEvent::ConnectionAdded {
                connection_ref_json,
            }
            | DomainEvent::ConnectionRemoved {
                connection_ref_json,
            }
            | DomainEvent::ConnectionUpdated {
                connection_ref_json,
            } => match serde_json::from_value::<ConnectionRef>(connection_ref_json.clone()) {
                Ok(r) => r,
                Err(err) => {
                    tracing::warn!(
                        target: "workflows-bus",
                        "[workflows-bus] could not decode ConnectionRef payload: {err}; skipping"
                    );
                    return;
                }
            },
            _ => return,
        };
        recompute_for_ref(&self.config, &r#ref).await;
    }
}

/// Run the bounded recompute pass for one connection ref. Public so
/// tests can drive it directly without going through the global bus.
pub async fn recompute_for_ref(config: &Config, r#ref: &ConnectionRef) {
    tracing::debug!(
        target: "workflows-bus",
        "[workflows-bus] recompute fired ref={ref:?}"
    );

    // Phase 1: workflows referencing this connection are bounded
    // (single user, O(10s) of workflows). The LIKE pre-filter narrows
    // the candidate set to O(refs-per-workflow); the recompute pass
    // verifies via `referenced_connections` so an over-selecting LIKE
    // does no harm.
    let affected = match store::list_workflows_referencing(config, r#ref) {
        Ok(rows) => rows,
        Err(err) => {
            tracing::error!(
                target: "workflows-bus",
                "[workflows-bus] list_workflows_referencing failed: {err:#}"
            );
            return;
        }
    };
    if affected.is_empty() {
        return;
    }

    let snapshot = match aggregator::list_all(config).await {
        Ok(views) => ConnectionsSnapshot::new(views),
        Err(err) => {
            tracing::warn!(
                target: "workflows-bus",
                "[workflows-bus] aggregator::list_all failed: {err:#}; using empty snapshot"
            );
            ConnectionsSnapshot::empty()
        }
    };

    for wf in affected {
        // Second-pass filter: the LIKE pre-filter may have caught
        // workflows that don't actually reference this ref (collision
        // on the JSON fragment). Verify before recomputing.
        let referenced = health::referenced_connections(&wf);
        if !referenced.contains(r#ref) {
            tracing::trace!(
                target: "workflows-bus",
                "[workflows-bus] skipping wf={} — LIKE pre-filter false-positive",
                wf.id
            );
            continue;
        }

        let new_health = health::recompute(&wf, &snapshot);
        if new_health == wf.health {
            // No transition — skip the UPDATE and the bus publish so
            // subscribers don't see redundant events.
            tracing::trace!(
                target: "workflows-bus",
                "[workflows-bus] no transition wf={} health={:?}",
                wf.id,
                wf.health
            );
            continue;
        }

        let now = chrono::Utc::now();
        match store::set_health(config, &wf.id, &new_health, now) {
            Ok(true) => {
                tracing::info!(
                    target: "workflows-bus",
                    "[workflows-bus] health changed wf={} old={:?} new={:?}",
                    wf.id,
                    wf.health,
                    new_health
                );
                let health_json =
                    serde_json::to_value(&new_health).unwrap_or(serde_json::Value::Null);
                publish_global(DomainEvent::WorkflowHealthChanged {
                    workflow_id: wf.id.clone(),
                    health_json,
                });
            }
            Ok(false) => {
                // Workflow was deleted between list + set_health. Acceptable.
                tracing::debug!(
                    target: "workflows-bus",
                    "[workflows-bus] set_health missed wf={} (deleted mid-recompute)",
                    wf.id
                );
            }
            Err(err) => {
                tracing::error!(
                    target: "workflows-bus",
                    "[workflows-bus] set_health failed wf={}: {err:#}",
                    wf.id
                );
            }
        }
    }
}

/// Walk every workflow and recompute its health against the live
/// connections snapshot, persisting + publishing only when the
/// computed value actually changes.
///
/// Use case: a fix to the health-matching logic (e.g. F-15's
/// wildcard `account_id` / `channel_id` semantics) needs to roll
/// forward against already-persisted workflows. The per-connection
/// recompute path only fires on `ConnectionAdded` /
/// `ConnectionRemoved` events; a workflow saved before the fix
/// keeps its stale `NeedsConnections` health until something
/// triggers a recompute. This helper runs at boot to flush stale
/// state forward.
pub async fn recompute_all_workflows(config: &Config) {
    let workflows = match store::list_workflows(
        config,
        &crate::openhuman::workflows::types::ListFilter::default(),
    ) {
        Ok(rows) => rows,
        Err(err) => {
            tracing::error!(
                target: "workflows-bus",
                "[workflows-bus] list_workflows failed during boot recompute: {err:#}"
            );
            return;
        }
    };
    if workflows.is_empty() {
        return;
    }
    let snapshot = match aggregator::list_all(config).await {
        Ok(views) => ConnectionsSnapshot::new(views),
        Err(err) => {
            tracing::warn!(
                target: "workflows-bus",
                "[workflows-bus] aggregator::list_all failed during boot recompute: {err:#}; skipping"
            );
            return;
        }
    };
    let mut updated = 0u32;
    let mut total = 0u32;
    for wf in workflows {
        total += 1;
        let new_health = health::recompute(&wf, &snapshot);
        if new_health == wf.health {
            continue;
        }
        let now = chrono::Utc::now();
        match store::set_health(config, &wf.id, &new_health, now) {
            Ok(true) => {
                updated += 1;
                tracing::info!(
                    target: "workflows-bus",
                    "[workflows-bus] boot recompute wf={} old={:?} new={:?}",
                    wf.id, wf.health, new_health
                );
                let health_json =
                    serde_json::to_value(&new_health).unwrap_or(serde_json::Value::Null);
                publish_global(DomainEvent::WorkflowHealthChanged {
                    workflow_id: wf.id.clone(),
                    health_json,
                });
            }
            Ok(false) => {}
            Err(err) => {
                tracing::error!(
                    target: "workflows-bus",
                    "[workflows-bus] boot recompute set_health failed wf={}: {err:#}",
                    wf.id
                );
            }
        }
    }
    tracing::info!(
        target: "workflows-bus",
        "[workflows-bus] boot recompute done: {updated}/{total} workflows transitioned"
    );
}

// ── F2-10: composio_event trigger subscriber ──────────────────────────

/// Boot-time registration helper for [`ComposioEventSubscriber`].
/// Subscribes to the global bus and leaks the handle so the background
/// task lives for the lifetime of the process. Idempotent at the
/// subscribe layer (the bus de-dupes by name), but typical callers
/// only invoke this once during core boot.
pub fn register_composio_event_subscriber(config: Arc<Config>) {
    let subscriber = Arc::new(ComposioEventSubscriber::new(config));
    match subscribe_global(subscriber) {
        Some(handle) => {
            tracing::info!(
                target: "workflows-bus",
                "[workflows-bus] registered composio-event subscriber"
            );
            std::mem::forget(handle);
        }
        None => {
            log::warn!(
                "[event_bus] failed to register workflows composio-event subscriber — bus not initialized"
            );
        }
    }
}

/// Listens for [`DomainEvent::ComposioTriggerReceived`] and dispatches
/// every enabled workflow whose `Trigger::ComposioEvent { toolkit,
/// trigger_id }` matches the event's `toolkit` + `trigger`. Multiple
/// workflows can match the same event (fan-out).
///
/// Single-flight (ADR-014) is enforced by [`executor::dispatch_run_with_payload`]:
/// if a run is already in-flight for one of the matched workflows, the
/// second dispatch publishes `WorkflowRunSkipped` and returns
/// `AlreadyRunning`. The subscriber treats `AlreadyRunning` as expected
/// and logs at `debug`; every other dispatch error is logged at `warn`.
pub struct ComposioEventSubscriber {
    config: Arc<Config>,
}

impl ComposioEventSubscriber {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl EventHandler for ComposioEventSubscriber {
    fn name(&self) -> &str {
        "workflow::composio_event"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["composio"])
    }

    async fn handle(&self, event: &DomainEvent) {
        let DomainEvent::ComposioTriggerReceived {
            toolkit,
            trigger,
            payload,
            ..
        } = event
        else {
            return;
        };

        dispatch_matching_workflows(&self.config, toolkit, trigger, payload.clone()).await;
    }
}

/// Resolve the workflows that match (toolkit, trigger) and fan out a
/// `dispatch_run_with_payload` call per match. Public so tests can
/// drive it directly without going through the global bus.
pub async fn dispatch_matching_workflows(
    config: &Config,
    toolkit: &str,
    trigger: &str,
    payload: serde_json::Value,
) {
    tracing::debug!(
        target: "workflows-bus",
        "[workflows-bus] composio trigger fan-out toolkit={toolkit} trigger={trigger}"
    );

    let matches = match store::list_workflows_matching_composio_event(config, toolkit, trigger) {
        Ok(rows) => rows,
        Err(err) => {
            tracing::error!(
                target: "workflows-bus",
                "[workflows-bus] list_workflows_matching_composio_event failed: {err:#}"
            );
            return;
        }
    };
    if matches.is_empty() {
        return;
    }

    for wf in matches {
        let wf_id = wf.id.clone();
        let payload_clone = payload.clone();
        // Owned `Config` clone so the spawned task doesn't borrow `self`.
        let cfg = config.clone();
        tokio::spawn(async move {
            match executor::dispatch_run_with_payload(
                &cfg,
                wf_id.clone(),
                TriggerSource::ComposioEvent,
                Some(payload_clone),
            )
            .await
            {
                Ok(run_id) => {
                    tracing::info!(
                        target: "workflows-bus",
                        "[workflows-bus] composio fan-out wf={wf_id} run={run_id} dispatched"
                    );
                }
                Err(err) => {
                    // Single-flight skip is expected on bursty events;
                    // executor already publishes `WorkflowRunSkipped`.
                    let msg = format!("{err:#}");
                    if msg.contains("AlreadyRunning") {
                        tracing::debug!(
                            target: "workflows-bus",
                            "[workflows-bus] composio fan-out wf={wf_id} skipped (already running)"
                        );
                    } else {
                        tracing::warn!(
                            target: "workflows-bus",
                            "[workflows-bus] composio fan-out wf={wf_id} dispatch failed: {msg}"
                        );
                    }
                }
            }
        });
    }
}
