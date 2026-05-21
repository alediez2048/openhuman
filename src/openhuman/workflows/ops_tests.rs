//! F-2 CRUD ops tests: round-trip every operation against an isolated
//! workspace + assert event-bus emissions on each mutation.

use super::ops;
use super::types::*;
use crate::core::event_bus::{init_global, subscribe_global, DomainEvent, EventHandler};
use crate::openhuman::config::Config;
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

fn sample_create(origin: WorkflowOrigin) -> CreateWorkflowRequest {
    CreateWorkflowRequest {
        name: "Morning digest".into(),
        description: Some("Test description".into()),
        trigger: Trigger::Manual,
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "do the thing".into(),
                allowed_connections: vec![],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
        }],
        edges: vec![],
        settings: None,
        origin,
    }
}

/// Test handler that pipes every `Workflow*` event into an mpsc channel
/// for assertion. Drop the [`SubscriptionHandle`] returned by
/// `subscribe_global` to unsubscribe.
struct EventProbe {
    tx: UnboundedSender<DomainEvent>,
}
#[async_trait::async_trait]
impl EventHandler for EventProbe {
    fn name(&self) -> &str {
        "workflows_ops_test_probe"
    }
    fn domains(&self) -> Option<&[&str]> {
        Some(&["workflow"])
    }
    async fn handle(&self, event: &DomainEvent) {
        let _ = self.tx.send(event.clone());
    }
}

/// Serialise tests that depend on the global bus so probes don't see
/// each other's events. We could create an isolated bus per test, but
/// `subscribe_global` is the production code path and matches reality.
static EVENT_LOCK: Mutex<()> = Mutex::new(());

fn setup_event_probe() -> (
    std::sync::MutexGuard<'static, ()>,
    tokio::sync::mpsc::UnboundedReceiver<DomainEvent>,
    Option<crate::core::event_bus::SubscriptionHandle>,
) {
    // Poisoned-mutex recovery so a panicking earlier test doesn't take
    // every subsequent run with it.
    let lock = EVENT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init_global(crate::core::event_bus::DEFAULT_CAPACITY);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = subscribe_global(Arc::new(EventProbe { tx }));
    (lock, rx, handle)
}

/// Drain events from the probe channel until one matches `predicate`.
/// Used in place of bare `rx.recv()` so we don't fail the assertion
/// when an event from a concurrently-aborting subscriber (or from a
/// no-op idempotent path) sneaks into the queue.
async fn await_matching(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DomainEvent>,
    expected_id: &str,
    predicate: impl Fn(&DomainEvent) -> bool,
) -> DomainEvent {
    tokio::time::timeout(std::time::Duration::from_secs(2), async move {
        loop {
            let event = rx.recv().await.expect("event channel open");
            if predicate(&event) {
                return event;
            }
            tracing::debug!(
                target: "workflows-test",
                "skipping unrelated event while waiting for {expected_id}: {event:?}"
            );
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no matching event arrived for {expected_id}"))
}

#[tokio::test]
async fn create_round_trips_with_user_chat_origin() {
    let (_dir, config) = config_with_temp_workspace();
    let (_lock, mut rx, _handle) = setup_event_probe();

    let req = sample_create(WorkflowOrigin::UserChat);
    let outcome = ops::create(&config, req).await.unwrap();
    let created = outcome.value;
    assert!(!created.id.is_empty());
    assert!(!created.enabled);
    assert_eq!(created.schema_version, 1);
    assert_eq!(created.origin, WorkflowOrigin::UserChat);
    assert_eq!(created.health, WorkflowHealth::Ready);

    // Re-fetch by id — round-trip through SQLite + JSON columns.
    let fetched = ops::get(&config, created.id.clone()).await.unwrap();
    assert_eq!(fetched.value, Some(created.clone()));

    // The mutation must have published `WorkflowDefined`.
    let event = await_matching(&mut rx, &created.id, |e| {
        matches!(e, DomainEvent::WorkflowDefined { workflow_id, .. } if workflow_id == &created.id)
    })
    .await;
    if let DomainEvent::WorkflowDefined { origin_json, .. } = event {
        assert_eq!(origin_json["type"], "user_chat");
    }
}

#[tokio::test]
async fn create_preserves_seed_template_id_origin() {
    let (_dir, config) = config_with_temp_workspace();
    let req = sample_create(WorkflowOrigin::Seed {
        template_id: "ru-1-founder-morning-digest".into(),
    });
    let created = ops::create(&config, req).await.unwrap().value;
    let fetched = ops::get(&config, created.id.clone())
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        fetched.origin,
        WorkflowOrigin::Seed {
            template_id: "ru-1-founder-morning-digest".into()
        }
    );
}

#[tokio::test]
async fn create_rejects_imported_origin() {
    let (_dir, config) = config_with_temp_workspace();
    let req = sample_create(WorkflowOrigin::Imported);
    let err = ops::create(&config, req).await.unwrap_err();
    assert!(
        err.to_string().contains("Imported"),
        "expected Imported rejection; got: {err}"
    );
}

#[tokio::test]
async fn create_rejects_empty_name() {
    let (_dir, config) = config_with_temp_workspace();
    let mut req = sample_create(WorkflowOrigin::UserChat);
    req.name = "   ".into();
    let err = ops::create(&config, req).await.unwrap_err();
    assert!(err.to_string().contains("name"));
}

#[tokio::test]
async fn create_rejects_empty_nodes() {
    let (_dir, config) = config_with_temp_workspace();
    let mut req = sample_create(WorkflowOrigin::UserChat);
    req.nodes = vec![];
    let err = ops::create(&config, req).await.unwrap_err();
    assert!(err.to_string().contains("nodes"));
}

#[tokio::test]
async fn get_returns_none_for_unknown_id() {
    let (_dir, config) = config_with_temp_workspace();
    let fetched = ops::get(&config, "no-such-id".into()).await.unwrap();
    assert!(fetched.value.is_none());
}

#[tokio::test]
async fn list_returns_rows_sorted_by_updated_at_desc() {
    let (_dir, config) = config_with_temp_workspace();
    let a = ops::create(&config, sample_create(WorkflowOrigin::UserChat))
        .await
        .unwrap()
        .value;
    // Force a non-zero gap so updated_at ordering is deterministic.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let b = ops::create(&config, sample_create(WorkflowOrigin::UserChat))
        .await
        .unwrap()
        .value;

    let rows = ops::list(&config, ListFilter::default())
        .await
        .unwrap()
        .value;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, b.id, "newest workflow must come first");
    assert_eq!(rows[1].id, a.id);
}

#[tokio::test]
async fn list_honors_enabled_filter() {
    let (_dir, config) = config_with_temp_workspace();
    let a = ops::create(&config, sample_create(WorkflowOrigin::UserChat))
        .await
        .unwrap()
        .value;
    let _b = ops::create(&config, sample_create(WorkflowOrigin::UserChat))
        .await
        .unwrap();
    ops::enable(&config, a.id.clone()).await.unwrap();

    let only_enabled = ops::list(
        &config,
        ListFilter {
            enabled: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .value;
    assert_eq!(only_enabled.len(), 1);
    assert_eq!(only_enabled[0].id, a.id);

    let only_disabled = ops::list(
        &config,
        ListFilter {
            enabled: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .value;
    assert_eq!(only_disabled.len(), 1);
}

#[tokio::test]
async fn list_honors_search_substring() {
    let (_dir, config) = config_with_temp_workspace();
    let mut req_a = sample_create(WorkflowOrigin::UserChat);
    req_a.name = "Founder daily brief".into();
    let _a = ops::create(&config, req_a).await.unwrap();
    let mut req_b = sample_create(WorkflowOrigin::UserChat);
    req_b.name = "Cold coffee reminder".into();
    let _b = ops::create(&config, req_b).await.unwrap();

    let hits = ops::list(
        &config,
        ListFilter {
            search: Some("FOUNDER".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .value;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "Founder daily brief");
}

#[tokio::test]
async fn update_applies_patches_and_bumps_updated_at() {
    let (_dir, config) = config_with_temp_workspace();
    let (_lock, mut rx, _handle) = setup_event_probe();

    let created = ops::create(&config, sample_create(WorkflowOrigin::UserChat))
        .await
        .unwrap()
        .value;
    let initial_updated = created.updated_at;

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let patched = ops::update(
        &config,
        UpdateWorkflowRequest {
            id: created.id.clone(),
            patches: WorkflowPatch {
                name: Some("Renamed".into()),
                description: Some(None),
                ..Default::default()
            },
        },
    )
    .await
    .unwrap()
    .value;

    assert_eq!(patched.id, created.id);
    assert_eq!(patched.name, "Renamed");
    assert_eq!(patched.description, None);
    assert!(patched.updated_at > initial_updated, "updated_at bumped");

    await_matching(
        &mut rx,
        &created.id,
        |e| matches!(e, DomainEvent::WorkflowUpdated { workflow_id } if workflow_id == &created.id),
    )
    .await;
}

#[tokio::test]
async fn update_rejects_unknown_id() {
    let (_dir, config) = config_with_temp_workspace();
    let err = ops::update(
        &config,
        UpdateWorkflowRequest {
            id: "no-such-id".into(),
            patches: WorkflowPatch {
                name: Some("x".into()),
                ..Default::default()
            },
        },
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn enable_flips_bit_emits_event_idempotent_no_op_publishes_nothing() {
    let (_dir, config) = config_with_temp_workspace();
    let (_lock, mut rx, _handle) = setup_event_probe();

    let created = ops::create(&config, sample_create(WorkflowOrigin::UserChat))
        .await
        .unwrap()
        .value;

    let enabled = ops::enable(&config, created.id.clone())
        .await
        .unwrap()
        .value;
    assert!(enabled.enabled);
    await_matching(
        &mut rx,
        &created.id,
        |e| matches!(e, DomainEvent::WorkflowEnabled { workflow_id } if workflow_id == &created.id),
    )
    .await;

    // Second enable is a no-op — must NOT publish another WorkflowEnabled
    // for this id.
    let already = ops::enable(&config, created.id.clone())
        .await
        .unwrap()
        .value;
    assert!(already.enabled);
    let next = tokio::time::timeout(std::time::Duration::from_millis(80), async {
        loop {
            let e = rx.recv().await?;
            if matches!(&e, DomainEvent::WorkflowEnabled { workflow_id } if workflow_id == &created.id)
            {
                return Some(e);
            }
        }
    })
    .await;
    assert!(
        matches!(next, Err(_) | Ok(None)),
        "no WorkflowEnabled event should be published on a no-op enable; got {next:?}"
    );
}

#[tokio::test]
async fn disable_flips_bit_emits_event() {
    let (_dir, config) = config_with_temp_workspace();
    let (_lock, mut rx, _handle) = setup_event_probe();

    let created = ops::create(&config, sample_create(WorkflowOrigin::UserChat))
        .await
        .unwrap()
        .value;
    ops::enable(&config, created.id.clone()).await.unwrap();

    let disabled = ops::disable(&config, created.id.clone())
        .await
        .unwrap()
        .value;
    assert!(!disabled.enabled);
    await_matching(&mut rx, &created.id, |e| {
        matches!(e, DomainEvent::WorkflowDisabled { workflow_id } if workflow_id == &created.id)
    })
    .await;
}

#[tokio::test]
async fn delete_cascades_runs_and_publishes_event() {
    let (_dir, config) = config_with_temp_workspace();
    let (_lock, mut rx, _handle) = setup_event_probe();

    let created = ops::create(&config, sample_create(WorkflowOrigin::UserChat))
        .await
        .unwrap()
        .value;

    // Insert a fake run via the store's connection so we can verify the
    // FK cascade fires on workflow deletion (full run-row CRUD lands in
    // F-8; for now we hit the raw SQL).
    super::store::with_connection(&config, |conn| {
        conn.execute(
            "INSERT INTO workflow_runs (id, workflow_id, trigger_source, status, started_at) \
             VALUES ('run-1', ?1, '{\"type\":\"manual\",\"initiator\":\"test\"}', 'running', ?2)",
            rusqlite::params![created.id, "2026-05-20T00:00:00Z"],
        )?;
        Ok(())
    })
    .unwrap();

    let removed = ops::delete(&config, created.id.clone())
        .await
        .unwrap()
        .value;
    assert!(removed);

    await_matching(
        &mut rx,
        &created.id,
        |e| matches!(e, DomainEvent::WorkflowDeleted { workflow_id } if workflow_id == &created.id),
    )
    .await;

    // FK cascade must have dropped the run row.
    let surviving_runs: i64 = super::store::with_connection(&config, |conn| {
        Ok(conn
            .query_row(
                "SELECT COUNT(*) FROM workflow_runs WHERE workflow_id = ?1",
                rusqlite::params![created.id],
                |row| row.get(0),
            )
            .unwrap())
    })
    .unwrap();
    assert_eq!(surviving_runs, 0);
}

#[tokio::test]
async fn delete_on_unknown_id_is_idempotent() {
    let (_dir, config) = config_with_temp_workspace();
    let removed = ops::delete(&config, "no-such-id".into())
        .await
        .unwrap()
        .value;
    assert!(!removed);
}

// ── F-5: list_starter_templates ────────────────────────────────────────

#[tokio::test]
async fn list_starter_templates_returns_all_four_on_a_fresh_workspace() {
    let (_dir, config) = config_with_temp_workspace();
    let views = ops::list_starter_templates(&config, Some(1))
        .await
        .unwrap()
        .value;
    assert_eq!(views.len(), 4);
    let ids: std::collections::HashSet<_> = views.iter().map(|v| v.template_id.clone()).collect();
    assert!(ids.contains("ru-1-founder-morning-digest"));
    assert!(ids.contains("ru-2-linkedin-engagement-queue"));
    assert!(ids.contains("ru-3-spotify-friday-five"));
    assert!(ids.contains("ru-4-jira-sprint-retro"));
}

#[tokio::test]
async fn list_starter_templates_dedups_added_seed_workflows() {
    let (_dir, config) = config_with_temp_workspace();

    // Promote RU-1 into the user's table by mimicking what F-6's [Add]
    // button will do: create a workflow with origin = Seed{ru-1}.
    let mut req = sample_create(WorkflowOrigin::Seed {
        template_id: "ru-1-founder-morning-digest".into(),
    });
    req.name = "Founder morning digest".into();
    ops::create(&config, req).await.unwrap();

    let views = ops::list_starter_templates(&config, Some(1))
        .await
        .unwrap()
        .value;
    assert_eq!(views.len(), 3, "RU-1 must be deduped out after [Add]");
    assert!(views
        .iter()
        .all(|v| v.template_id != "ru-1-founder-morning-digest"));
}

#[tokio::test]
async fn list_starter_templates_re_includes_deleted_templates() {
    let (_dir, config) = config_with_temp_workspace();
    let req = sample_create(WorkflowOrigin::Seed {
        template_id: "ru-2-linkedin-engagement-queue".into(),
    });
    let created = ops::create(&config, req).await.unwrap().value;
    assert_eq!(
        ops::list_starter_templates(&config, Some(1))
            .await
            .unwrap()
            .value
            .len(),
        3
    );

    ops::delete(&config, created.id.clone()).await.unwrap();
    assert_eq!(
        ops::list_starter_templates(&config, Some(1))
            .await
            .unwrap()
            .value
            .len(),
        4,
        "deleted seed workflows release their template back into the catalog"
    );
}

#[tokio::test]
async fn list_starter_templates_honors_min_phase_filter() {
    let (_dir, config) = config_with_temp_workspace();
    // Every Phase-1 template has min_phase=1; phase=0 must filter them all.
    let views = ops::list_starter_templates(&config, Some(0))
        .await
        .unwrap()
        .value;
    assert!(views.is_empty());
}

#[tokio::test]
async fn list_starter_templates_populates_missing_connections() {
    let (_dir, config) = config_with_temp_workspace();
    // Test workspace has no connections registered, so every required
    // connection must surface in missing_connections.
    let views = ops::list_starter_templates(&config, Some(1))
        .await
        .unwrap()
        .value;
    for v in views {
        assert!(
            !v.missing_connections.is_empty(),
            "template `{}` should report missing connections on an empty workspace",
            v.template_id
        );
        assert_eq!(
            v.missing_connections.len(),
            v.required_connections.len(),
            "with zero user connections, every required must be missing"
        );
    }
}

#[tokio::test]
async fn list_starter_templates_carries_trigger_summary_and_raw_payload() {
    let (_dir, config) = config_with_temp_workspace();
    let views = ops::list_starter_templates(&config, Some(1))
        .await
        .unwrap()
        .value;
    for v in views {
        assert!(
            !v.trigger_summary.is_empty(),
            "template `{}` must populate trigger_summary",
            v.template_id
        );
        assert!(
            v.raw_payload.is_object(),
            "template `{}` raw_payload must be a JSON object (consumed by workflows_create)",
            v.template_id
        );
    }
}
