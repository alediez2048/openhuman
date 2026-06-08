//! F-7 scheduler tests — exercises the in-memory registry +
//! `handle_run_now` against an isolated workspace. The polling loop
//! ([`scheduler::run`]) is not directly tested because it requires
//! tokio time-mocking — F-15's hero E2E walks the live cron tick.

use super::ops;
use super::scheduler;
use super::store;
use super::types::*;
use crate::openhuman::config::Config;
use chrono::TimeZone;
use std::sync::Mutex;
use tempfile::TempDir;

fn config_with_temp_workspace() -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, config)
}

fn workflow_with(id: &str, enabled: bool, trigger: Trigger, health: WorkflowHealth) -> Workflow {
    Workflow {
        id: id.into(),
        schema_version: 1,
        name: id.into(),
        description: None,
        enabled,
        origin: WorkflowOrigin::UserChat,
        health,
        trigger,
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "x".into(),
                allowed_connections: vec![],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
            retry_policy: None,
        }],
        edges: vec![],
        settings: WorkflowSettings::default(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        last_run_at: None,
    campaign_id: None,
    }
}

/// Tests touch the process-global registry, so serialize them.
static SCHEDULER_LOCK: Mutex<()> = Mutex::new(());

fn scheduler_test_lock() -> std::sync::MutexGuard<'static, ()> {
    let guard = SCHEDULER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    scheduler::reset_registry_for_test();
    guard
}

fn cron_trigger() -> Trigger {
    Trigger::Cron {
        expr: "0 8 * * 1-5".into(),
        tz: None,
        active_hours: None,
    }
}

#[test]
fn register_inserts_a_cron_workflow_into_the_registry() {
    let _lock = scheduler_test_lock();
    let wf = workflow_with("wf-cron-a", true, cron_trigger(), WorkflowHealth::Ready);
    let next = scheduler::register(&wf).unwrap();
    assert!(
        next.is_some(),
        "register on a cron+enabled workflow must succeed"
    );
    assert_eq!(scheduler::registered_ids_for_test(), vec!["wf-cron-a"]);
}

#[test]
fn register_skips_manual_triggers() {
    let _lock = scheduler_test_lock();
    let wf = workflow_with("wf-manual", true, Trigger::Manual, WorkflowHealth::Ready);
    let next = scheduler::register(&wf).unwrap();
    assert!(next.is_none(), "manual trigger must not produce a next_run");
    assert!(scheduler::registered_ids_for_test().is_empty());
}

#[test]
fn register_skips_disabled_workflows() {
    let _lock = scheduler_test_lock();
    let wf = workflow_with("wf-disabled", false, cron_trigger(), WorkflowHealth::Ready);
    let next = scheduler::register(&wf).unwrap();
    assert!(next.is_none());
    assert!(scheduler::registered_ids_for_test().is_empty());
}

#[test]
fn deregister_removes_a_registered_workflow() {
    let _lock = scheduler_test_lock();
    let wf = workflow_with("wf-cron-b", true, cron_trigger(), WorkflowHealth::Ready);
    scheduler::register(&wf).unwrap();
    assert_eq!(scheduler::registered_ids_for_test(), vec!["wf-cron-b"]);
    scheduler::deregister(&wf.id);
    assert!(scheduler::registered_ids_for_test().is_empty());
}

#[test]
fn deregister_is_idempotent() {
    let _lock = scheduler_test_lock();
    // Removing an unknown id is a silent no-op.
    scheduler::deregister(&"no-such-id".to_string());
    assert!(scheduler::registered_ids_for_test().is_empty());
}

#[tokio::test]
async fn reconcile_at_startup_registers_only_enabled_cron_workflows() {
    let _lock = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    // 5 workflows: 3 enabled+cron, 1 enabled+manual, 1 disabled+cron.
    let fixtures = [
        ("wf-a", true, cron_trigger()),
        ("wf-b", true, cron_trigger()),
        ("wf-c", true, cron_trigger()),
        ("wf-d", true, Trigger::Manual),
        ("wf-e", false, cron_trigger()),
    ];
    for (id, enabled, trig) in fixtures {
        let wf = workflow_with(id, enabled, trig, WorkflowHealth::Ready);
        store::insert_workflow(&config, &wf).unwrap();
    }
    let count = scheduler::reconcile_at_startup(&config).await.unwrap();
    assert_eq!(count, 3);
    let ids = scheduler::registered_ids_for_test();
    assert_eq!(ids, vec!["wf-a", "wf-b", "wf-c"]);
}

#[tokio::test]
async fn handle_run_now_on_ready_workflow_returns_a_run_id() {
    let _lock = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    let wf = workflow_with("wf-run", false, Trigger::Manual, WorkflowHealth::Ready);
    store::insert_workflow(&config, &wf).unwrap();
    let run_id = scheduler::handle_run_now(&config, wf.id.clone(), ManualInitiator::User)
        .await
        .unwrap();
    assert!(!run_id.is_empty());
}

#[tokio::test]
async fn handle_run_now_rejects_unhealthy_workflows() {
    let _lock = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    let wf = workflow_with(
        "wf-unhealthy",
        true,
        cron_trigger(),
        WorkflowHealth::NeedsConnections {
            missing: vec![
                crate::openhuman::connections::types::ConnectionRef::Composio {
                    toolkit_id: "gmail".into(),
                    account_id: None,
                },
            ],
        },
    );
    store::insert_workflow(&config, &wf).unwrap();
    let err = scheduler::handle_run_now(&config, wf.id, ManualInitiator::User)
        .await
        .unwrap_err();
    assert!(matches!(err, RunNowError::HealthBlocked { .. }));
    assert_eq!(err.code(), "health_blocked");
}

#[tokio::test]
async fn handle_run_now_returns_not_found_on_unknown_id() {
    let _lock = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    let err = scheduler::handle_run_now(&config, "no-such-id".into(), ManualInitiator::User)
        .await
        .unwrap_err();
    assert!(matches!(err, RunNowError::NotFound));
    assert_eq!(err.code(), "not_found");
}

#[tokio::test]
async fn ops_create_with_enabled_false_does_not_register() {
    let _lock = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    let req = CreateWorkflowRequest {
        name: "test".into(),
        description: None,
        trigger: cron_trigger(),
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "x".into(),
                allowed_connections: vec![],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
            retry_policy: None,
        }],
        edges: vec![],
        settings: None,
        origin: WorkflowOrigin::UserChat,
    };
    let _ = ops::create(&config, req).await.unwrap();
    // ops::create defaults to enabled=false → scheduler must skip.
    assert!(scheduler::registered_ids_for_test().is_empty());
}

#[tokio::test]
async fn enable_registers_and_disable_deregisters() {
    let _lock = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    let req = CreateWorkflowRequest {
        name: "test".into(),
        description: None,
        trigger: cron_trigger(),
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "x".into(),
                allowed_connections: vec![],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
            retry_policy: None,
        }],
        edges: vec![],
        settings: None,
        origin: WorkflowOrigin::UserChat,
    };
    let created = ops::create(&config, req).await.unwrap().value;
    assert!(scheduler::registered_ids_for_test().is_empty());

    ops::enable(&config, created.id.clone()).await.unwrap();
    assert_eq!(
        scheduler::registered_ids_for_test(),
        vec![created.id.clone()]
    );

    ops::disable(&config, created.id.clone()).await.unwrap();
    assert!(scheduler::registered_ids_for_test().is_empty());
}

#[tokio::test]
async fn delete_deregisters_before_the_cascade() {
    let _lock = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    let wf = workflow_with("wf-del", true, cron_trigger(), WorkflowHealth::Ready);
    store::insert_workflow(&config, &wf).unwrap();
    scheduler::register(&wf).unwrap();
    assert_eq!(scheduler::registered_ids_for_test(), vec!["wf-del"]);

    ops::delete(&config, wf.id.clone()).await.unwrap();
    assert!(scheduler::registered_ids_for_test().is_empty());
    assert!(store::get_workflow(&config, &wf.id).unwrap().is_none());
}

// ── F2-15: active_hours gate ───────────────────────────────────────────

fn cron_with_active_hours(start: &str, end: &str, tz: Option<&str>) -> Trigger {
    Trigger::Cron {
        expr: "* * * * *".into(),
        tz: tz.map(str::to_string),
        active_hours: Some(ActiveHours {
            start: start.into(),
            end: end.into(),
        }),
    }
}

#[test]
fn active_hours_gate_returns_none_when_no_active_hours_configured() {
    let _guard = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    let wf = workflow_with("wf-ah-none", true, cron_trigger(), WorkflowHealth::Ready);
    store::insert_workflow(&config, &wf).unwrap();
    // cron_trigger has active_hours = None; the gate MUST pass.
    let now = chrono::Utc::now();
    assert!(scheduler::active_hours_skip(&config, &wf.id, now).is_none());
}

#[test]
fn active_hours_gate_passes_when_now_is_inside_the_window_utc() {
    let _guard = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    let wf = workflow_with(
        "wf-ah-utc-inside",
        true,
        cron_with_active_hours("09:00", "17:00", None),
        WorkflowHealth::Ready,
    );
    store::insert_workflow(&config, &wf).unwrap();
    // Pick a UTC instant that's 12:00 UTC — squarely inside the window.
    let now = chrono::Utc.with_ymd_and_hms(2026, 5, 24, 12, 0, 0).unwrap();
    assert!(scheduler::active_hours_skip(&config, &wf.id, now).is_none());
}

#[test]
fn active_hours_gate_skips_when_now_is_outside_the_window_utc() {
    let _guard = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    let wf = workflow_with(
        "wf-ah-utc-outside",
        true,
        cron_with_active_hours("09:00", "17:00", None),
        WorkflowHealth::Ready,
    );
    store::insert_workflow(&config, &wf).unwrap();
    // 04:00 UTC — outside the [09:00, 17:00) window.
    let now = chrono::Utc.with_ymd_and_hms(2026, 5, 24, 4, 0, 0).unwrap();
    let skip = scheduler::active_hours_skip(&config, &wf.id, now)
        .expect("dispatch outside window MUST be gated");
    assert_eq!(skip.start, "09:00");
    assert_eq!(skip.end, "17:00");
    assert_eq!(skip.now_hhmm, "04:00");
}

#[test]
fn active_hours_gate_resolves_now_in_the_configured_timezone() {
    let _guard = scheduler_test_lock();
    let (_dir, config) = config_with_temp_workspace();
    let wf = workflow_with(
        "wf-ah-tz",
        true,
        cron_with_active_hours("09:00", "17:00", Some("America/Los_Angeles")),
        WorkflowHealth::Ready,
    );
    store::insert_workflow(&config, &wf).unwrap();

    // 17:00 UTC on 2026-05-24 = 10:00 PT (PDT, UTC-7). Inside [09:00, 17:00) PT.
    let pt_inside = chrono::Utc.with_ymd_and_hms(2026, 5, 24, 17, 0, 0).unwrap();
    assert!(
        scheduler::active_hours_skip(&config, &wf.id, pt_inside).is_none(),
        "10:00 PT MUST be inside the [09:00, 17:00) window"
    );

    // 04:00 UTC = 21:00 prior-day PT — outside the window.
    let pt_outside = chrono::Utc.with_ymd_and_hms(2026, 5, 25, 4, 0, 0).unwrap();
    let skip =
        scheduler::active_hours_skip(&config, &wf.id, pt_outside).expect("21:00 PT MUST be gated");
    assert_eq!(skip.now_hhmm, "21:00");
}

#[tokio::test]
async fn active_hours_publish_emits_skip_event_with_outside_active_hours_reason() {
    use crate::core::event_bus::{init_global, subscribe_global, DomainEvent, EventHandler};
    use std::sync::Arc;

    struct Probe(tokio::sync::mpsc::UnboundedSender<DomainEvent>);
    #[async_trait::async_trait]
    impl EventHandler for Probe {
        fn name(&self) -> &str {
            "scheduler_active_hours_probe"
        }
        fn domains(&self) -> Option<&[&str]> {
            Some(&["workflow"])
        }
        async fn handle(&self, event: &DomainEvent) {
            let _ = self.0.send(event.clone());
        }
    }

    let _guard = scheduler_test_lock();
    init_global(crate::core::event_bus::DEFAULT_CAPACITY);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let _handle = subscribe_global(Arc::new(Probe(tx)));

    let skip = scheduler::ActiveHoursSkip {
        start: "09:00".into(),
        end: "17:00".into(),
        now_hhmm: "04:00".into(),
    };
    scheduler::publish_active_hours_skip(&"wf-skip-evt".to_string(), &skip);

    for _ in 0..20 {
        if let Ok(Some(event)) =
            tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await
        {
            if let DomainEvent::WorkflowRunSkipped {
                workflow_id,
                reason_json,
                attempted_trigger_source_json,
            } = event
            {
                if workflow_id == "wf-skip-evt" {
                    assert_eq!(reason_json["kind"], "outside_active_hours");
                    assert_eq!(reason_json["start"], "09:00");
                    assert_eq!(reason_json["end"], "17:00");
                    assert_eq!(reason_json["now_hhmm"], "04:00");
                    assert_eq!(attempted_trigger_source_json["type"], "cron");
                    return;
                }
            }
        }
    }
    panic!("WorkflowRunSkipped event for wf-skip-evt never arrived");
}
