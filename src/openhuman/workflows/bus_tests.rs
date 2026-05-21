//! F-3 bus subscriber tests — drive the recompute pass directly via
//! `recompute_for_ref` rather than the global bus so we can test the
//! algorithm in isolation, without contention on the singleton
//! subscriber list. A separate integration-style test exercises the
//! full publish → subscriber chain.

use super::bus::{recompute_for_ref, WorkflowHealthRecomputeSubscriber};
use super::store;
use super::types::*;
use crate::core::event_bus::{init_global, subscribe_global, DomainEvent, EventHandler};
use crate::openhuman::config::Config;
use crate::openhuman::connections::types::ConnectionRef;
use chrono::Utc;
use std::sync::Arc;
use std::sync::Mutex;
use tempfile::TempDir;
use tokio::sync::mpsc::UnboundedSender;

fn config_with_temp_workspace() -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, config)
}

fn workflow_referencing(id: &str, r#ref: ConnectionRef, initial: WorkflowHealth) -> Workflow {
    Workflow {
        id: id.into(),
        schema_version: 1,
        name: format!("wf-{id}"),
        description: None,
        enabled: false,
        origin: WorkflowOrigin::UserChat,
        health: initial,
        trigger: Trigger::Manual,
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "x".into(),
                allowed_connections: vec![r#ref],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
        }],
        edges: vec![],
        settings: WorkflowSettings::default(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_run_at: None,
    }
}

#[tokio::test]
async fn recompute_for_ref_skips_when_no_workflows_reference_the_ref() {
    let (_dir, config) = config_with_temp_workspace();
    // Empty store — nothing to recompute. Must not panic / not crash.
    recompute_for_ref(
        &config,
        &ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        },
    )
    .await;
}

#[tokio::test]
async fn recompute_does_not_publish_when_state_does_not_change() {
    // Set up a workflow already marked Ready against a snapshot that
    // STILL counts as ready — recompute is a no-op transition.
    let (_dir, config) = config_with_temp_workspace();
    let (_lock, mut rx, _handle) = setup_event_probe();

    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    // Composio status alone is authoritative — so an empty aggregator
    // snapshot means the wf is NeedsConnections. Set initial to match
    // so there's no transition.
    let initial = WorkflowHealth::NeedsConnections {
        missing: vec![gmail.clone()],
    };
    let wf = workflow_referencing("wf-noop", gmail.clone(), initial.clone());
    store::insert_workflow(&config, &wf).unwrap();

    recompute_for_ref(&config, &gmail).await;

    // Persisted state still NeedsConnections — health column untouched.
    let after = store::get_workflow(&config, &"wf-noop".to_string())
        .unwrap()
        .unwrap();
    assert_eq!(after.health, initial);

    // No bus event for THIS workflow's no-op transition. (Other
    // tests may publish their own events on the shared global bus —
    // filter by id.)
    let next = tokio::time::timeout(std::time::Duration::from_millis(150), async {
        loop {
            let e = rx.recv().await?;
            if matches!(
                &e,
                DomainEvent::WorkflowHealthChanged { workflow_id, .. } if workflow_id == "wf-noop"
            ) {
                return Some(e);
            }
        }
    })
    .await;
    assert!(
        matches!(next, Err(_) | Ok(None)),
        "no WorkflowHealthChanged for wf-noop should publish on a no-op recompute; got {next:?}"
    );
}

// NB on the forward transition (NeedsConnections → Ready): the
// production `recompute_for_ref` calls `aggregator::list_all`, which
// runs through the real per-mechanism collectors. Mocking the
// aggregator is out of scope for F-3, so we exercise the reverse
// transition (Ready → NeedsConnections, against an empty aggregator
// output) below. F-15's hero E2E walks the full forward path against
// a real connection in a live build.

#[tokio::test]
async fn recompute_flips_ready_to_needs_connections_against_empty_snapshot() {
    let (_dir, config) = config_with_temp_workspace();
    let (_lock, mut rx, _handle) = setup_event_probe();

    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    let wf = workflow_referencing("wf-flip", gmail.clone(), WorkflowHealth::Ready);
    store::insert_workflow(&config, &wf).unwrap();

    recompute_for_ref(&config, &gmail).await;

    let after = store::get_workflow(&config, &"wf-flip".to_string())
        .unwrap()
        .unwrap();
    match &after.health {
        WorkflowHealth::NeedsConnections { missing } => {
            assert_eq!(missing, &vec![gmail.clone()]);
        }
        other => panic!("expected NeedsConnections, got {other:?}"),
    }

    let event = await_health_changed(&mut rx, "wf-flip").await;
    match event {
        DomainEvent::WorkflowHealthChanged {
            workflow_id,
            health_json,
        } => {
            assert_eq!(workflow_id, "wf-flip");
            assert_eq!(health_json["type"], "needs_connections");
        }
        other => panic!("expected WorkflowHealthChanged, got {other:?}"),
    }
}

#[tokio::test]
async fn recompute_is_bounded_only_touches_referencing_workflows() {
    // Insert 5 workflows; only 2 reference the changed connection.
    // recompute_for_ref must only set_health on those 2.
    let (_dir, config) = config_with_temp_workspace();

    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    let slack = ConnectionRef::Composio {
        toolkit_id: "slack".into(),
        account_id: None,
    };

    for (i, r) in [
        gmail.clone(),
        slack.clone(),
        gmail.clone(),
        slack.clone(),
        slack.clone(),
    ]
    .iter()
    .enumerate()
    {
        let wf = workflow_referencing(&format!("wf-{i}"), r.clone(), WorkflowHealth::Ready);
        store::insert_workflow(&config, &wf).unwrap();
    }

    // Run recompute for gmail — only wf-0 and wf-2 reference it.
    recompute_for_ref(&config, &gmail).await;

    let rows = store::list_workflows(&config, &ListFilter::default()).unwrap();
    for wf in rows {
        match wf.id.as_str() {
            "wf-0" | "wf-2" => {
                assert!(matches!(wf.health, WorkflowHealth::NeedsConnections { .. }))
            }
            other => assert_eq!(
                wf.health,
                WorkflowHealth::Ready,
                "wf {other} must be untouched"
            ),
        }
    }
}

#[tokio::test]
async fn recompute_filters_out_like_false_positives() {
    // Two workflows: one references {toolkit_id: "gmail"}, the other's
    // NAME contains the literal string "gmail" but its allowed_connections
    // does not. The LIKE pre-filter is matched against nodes_json (not
    // the name), so the second won't be a false positive — but a more
    // adversarial fixture (e.g. a description literally containing the
    // toolkit_id) might be. We verify the second-pass filter via
    // referenced_connections catches the trivial case.
    let (_dir, config) = config_with_temp_workspace();
    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    let referencing = workflow_referencing("wf-ref", gmail.clone(), WorkflowHealth::Ready);
    store::insert_workflow(&config, &referencing).unwrap();

    let mut unrelated = workflow_referencing(
        "wf-unrelated",
        ConnectionRef::Mcp {
            server_id: "higgsfield".into(),
            tool_name: None,
        },
        WorkflowHealth::Ready,
    );
    unrelated.name = "gmail-named workflow with no Composio".into();
    store::insert_workflow(&config, &unrelated).unwrap();

    recompute_for_ref(&config, &gmail).await;

    let ref_after = store::get_workflow(&config, &"wf-ref".to_string())
        .unwrap()
        .unwrap();
    assert!(matches!(
        ref_after.health,
        WorkflowHealth::NeedsConnections { .. }
    ));

    let unrelated_after = store::get_workflow(&config, &"wf-unrelated".to_string())
        .unwrap()
        .unwrap();
    assert_eq!(
        unrelated_after.health,
        WorkflowHealth::Ready,
        "unrelated workflow must NOT be touched"
    );
}

#[tokio::test]
async fn subscriber_handle_decodes_connection_ref_payload() {
    let (_dir, config) = config_with_temp_workspace();
    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    let wf = workflow_referencing("wf-handle", gmail.clone(), WorkflowHealth::Ready);
    store::insert_workflow(&config, &wf).unwrap();

    let subscriber = WorkflowHealthRecomputeSubscriber::new(Arc::new(config.clone()));

    // Drive `handle` directly with a synthetic ConnectionRemoved event.
    let event = DomainEvent::ConnectionRemoved {
        connection_ref_json: serde_json::to_value(&gmail).unwrap(),
    };
    subscriber.handle(&event).await;

    let after = store::get_workflow(&config, &"wf-handle".to_string())
        .unwrap()
        .unwrap();
    assert!(matches!(
        after.health,
        WorkflowHealth::NeedsConnections { .. }
    ));
}

#[tokio::test]
async fn subscriber_ignores_unknown_events() {
    let (_dir, config) = config_with_temp_workspace();
    let subscriber = WorkflowHealthRecomputeSubscriber::new(Arc::new(config.clone()));

    // Some unrelated event from a different domain.
    let event = DomainEvent::SystemStartup {
        component: "noise".into(),
    };
    // Must not panic / no-op.
    subscriber.handle(&event).await;
}

#[tokio::test]
async fn subscriber_recovers_from_unparseable_payload() {
    let (_dir, config) = config_with_temp_workspace();
    let subscriber = WorkflowHealthRecomputeSubscriber::new(Arc::new(config.clone()));

    // Payload is not a ConnectionRef shape.
    let event = DomainEvent::ConnectionAdded {
        connection_ref_json: serde_json::json!({ "totally": "not a ref" }),
    };
    subscriber.handle(&event).await; // must not panic
}

// ── event-bus probe scaffolding ─────────────────────────────────────────

struct EventProbe {
    tx: UnboundedSender<DomainEvent>,
}
#[async_trait::async_trait]
impl EventHandler for EventProbe {
    fn name(&self) -> &str {
        "workflows_bus_test_probe"
    }
    fn domains(&self) -> Option<&[&str]> {
        Some(&["workflow"])
    }
    async fn handle(&self, event: &DomainEvent) {
        let _ = self.tx.send(event.clone());
    }
}

static EVENT_LOCK: Mutex<()> = Mutex::new(());

fn setup_event_probe() -> (
    std::sync::MutexGuard<'static, ()>,
    tokio::sync::mpsc::UnboundedReceiver<DomainEvent>,
    Option<crate::core::event_bus::SubscriptionHandle>,
) {
    let lock = EVENT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init_global(crate::core::event_bus::DEFAULT_CAPACITY);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = subscribe_global(Arc::new(EventProbe { tx }));
    (lock, rx, handle)
}

async fn await_health_changed(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DomainEvent>,
    expected_id: &str,
) -> DomainEvent {
    tokio::time::timeout(std::time::Duration::from_secs(2), async move {
        loop {
            let event = rx.recv().await.expect("event channel open");
            if let DomainEvent::WorkflowHealthChanged { workflow_id, .. } = &event {
                if workflow_id == expected_id {
                    return event;
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no matching WorkflowHealthChanged arrived for {expected_id}"))
}
