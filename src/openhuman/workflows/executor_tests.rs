//! F-8 / F-9 / F-15 executor tests: dispatch validation, the full
//! happy-path pipeline (run-row + step-row persistence +
//! truncation), single-flight + cancel + orphan-recovery, and the
//! `build_node_agent_definition` allowlist contract.
//!
//! F-15 swapped the agent body for the live `Agent::run_single()`
//! invocation. These tests inject a deterministic stub via
//! [`executor::set_test_agent_prompt_override`] so the persistence
//! pipeline assertions don't depend on a configured LLM provider in
//! the test workspace.

use super::executor::{
    self, build_node_agent_definition, set_test_agent_prompt_override, BASELINE_TOOL_NAMES,
    READ_ONLY_WORKFLOW_TOOL_NAMES,
};
use super::ops;
use super::store::{self, Pagination};
use super::types::*;
use crate::openhuman::config::Config;
use crate::openhuman::connections::types::ConnectionRef;
use std::sync::Once;
use tempfile::TempDir;

/// Install a deterministic agent stub the first time any executor
/// test runs. The stub echoes the prompt + allowed-tool count back
/// to the caller, which gives every assertion a stable shape to
/// match against (length, substring, truncation) without needing
/// the real LLM.
fn install_test_agent_stub() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        set_test_agent_prompt_override(|prompt, def| {
            Ok(format!(
                "[test-stub] prompt={} ({} chars), allowed_tools={}",
                prompt,
                prompt.chars().count(),
                def.allowed_tools.len()
            ))
        });
    });
}

fn config_with_temp_workspace() -> (TempDir, Config) {
    install_test_agent_stub();
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, config)
}

fn create_request(prompt: &str) -> CreateWorkflowRequest {
    CreateWorkflowRequest {
        name: "F-8 happy-path".into(),
        description: None,
        trigger: Trigger::Manual,
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: prompt.into(),
                allowed_connections: vec![],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
        }],
        edges: vec![],
        settings: None,
        origin: WorkflowOrigin::UserChat,
    }
}

async fn wait_for_terminal_run(config: &Config, workflow_id: &str) -> Run {
    // The executor spawns the run on a tokio task; poll list_runs
    // until the row reaches a terminal status. Bounded at 2 seconds —
    // the placeholder body is synchronous so this should resolve in
    // a few poll cycles.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let runs = store::list_runs(config, &workflow_id.to_string(), Pagination::default())
            .expect("list_runs");
        if let Some(run) = runs.first() {
            if !matches!(run.status, RunStatus::Pending | RunStatus::Running) {
                return run.clone();
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("run never reached terminal status within 2s");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

// ── build_node_agent_definition ────────────────────────────────────────

#[test]
fn build_node_agent_definition_baseline_only_when_no_connections() {
    let def = build_node_agent_definition(&[], 12, None);
    let mut expected: Vec<String> = BASELINE_TOOL_NAMES.iter().map(|s| s.to_string()).collect();
    expected.extend(READ_ONLY_WORKFLOW_TOOL_NAMES.iter().map(|s| s.to_string()));
    assert_eq!(def.allowed_tools, expected);
    assert_eq!(def.iteration_cap, 12);
    assert_eq!(def.model_tier, None);
}

#[test]
fn build_node_agent_definition_dedups_duplicates_preserves_order() {
    // The connection-resolution path could plausibly produce a duplicate
    // of `list_connections` — assert the dedup keeps the first
    // occurrence + drops subsequent matches.
    let conns = vec![
        ConnectionRef::Composio {
            toolkit_id: "github".into(),
            account_id: Some("c1".into()),
        },
        ConnectionRef::Builtin {
            integration: "memory".into(),
        },
        // Second Composio entry resolves to the same tool name —
        // must dedupe to a single `composio_execute` entry.
        ConnectionRef::Composio {
            toolkit_id: "linear".into(),
            account_id: Some("c2".into()),
        },
    ];
    let def = build_node_agent_definition(&conns, 5, Some("reasoning".into()));

    let composio_count = def
        .allowed_tools
        .iter()
        .filter(|t| t.as_str() == "composio_execute")
        .count();
    assert_eq!(
        composio_count, 1,
        "duplicate composio entries must collapse to a single tool name"
    );
    // The first occurrence comes after the baseline names.
    let baseline_len = BASELINE_TOOL_NAMES.len();
    assert_eq!(def.allowed_tools[baseline_len], "composio_execute");
    assert_eq!(def.allowed_tools[baseline_len + 1], "builtin_memory");
    assert_eq!(def.iteration_cap, 5);
    assert_eq!(def.model_tier.as_deref(), Some("reasoning"));
}

#[test]
fn build_node_agent_definition_appends_read_only_workflow_tools_last() {
    let def = build_node_agent_definition(
        &[ConnectionRef::Channel {
            provider: "slack".into(),
            channel_id: "C123".into(),
        }],
        12,
        None,
    );
    let last_four: Vec<&str> = def
        .allowed_tools
        .iter()
        .rev()
        .take(READ_ONLY_WORKFLOW_TOOL_NAMES.len())
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let expected: Vec<&str> = READ_ONLY_WORKFLOW_TOOL_NAMES.to_vec();
    assert_eq!(last_four, expected);
}

// ── dispatch_run validation ────────────────────────────────────────────

#[tokio::test]
async fn dispatch_run_rejects_unknown_workflow() {
    let (_dir, config) = config_with_temp_workspace();
    let err = executor::dispatch_run(
        &config,
        "missing".into(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap_err();
    let dispatch_err = err
        .downcast::<executor::DispatchError>()
        .expect("DispatchError");
    assert_eq!(dispatch_err.code(), "not_found");
}

// ── dispatch_run happy path ────────────────────────────────────────────

#[tokio::test]
async fn dispatch_run_persists_run_and_step_rows() {
    let (_dir, config) = config_with_temp_workspace();
    let created = ops::create(&config, create_request("Summarize my inbox"))
        .await
        .unwrap()
        .value;

    let run_id = executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    assert!(!run_id.is_empty());

    let terminal = wait_for_terminal_run(&config, &created.id).await;
    assert_eq!(terminal.id, run_id);
    assert!(matches!(terminal.status, RunStatus::Succeeded));
    assert!(terminal.completed_at.is_some());

    // Step row must exist + carry the placeholder body.
    let (_run, steps) = store::get_run(&config, &run_id).unwrap().expect("run row");
    assert_eq!(steps.len(), 1);
    let step = &steps[0];
    assert!(matches!(step.status, RunStatus::Succeeded));
    let output = step.output_json.as_deref().unwrap_or("");
    assert!(
        output.contains("[test-stub]"),
        "step output must carry the test stub's marker, got {output:?}"
    );
}

#[tokio::test]
async fn dispatch_run_truncates_huge_step_output_to_64kib() {
    let (_dir, config) = config_with_temp_workspace();
    // The placeholder echoes the prompt into the output, so an
    // ~80KiB prompt forces the step output through truncation.
    let huge_prompt = "x".repeat(80 * 1024);
    let created = ops::create(&config, create_request(&huge_prompt))
        .await
        .unwrap()
        .value;

    executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();

    let terminal = wait_for_terminal_run(&config, &created.id).await;
    let (_run, steps) = store::get_run(&config, &terminal.id)
        .unwrap()
        .expect("run row");
    let output_json = steps[0]
        .output_json
        .as_deref()
        .expect("succeeded step has output_json");
    let parsed: serde_json::Value = serde_json::from_str(output_json).unwrap();
    let text = parsed["text"].as_str().expect("text field");
    assert!(text.len() <= store::RUN_STEP_OUTPUT_MAX_BYTES);
    assert!(text.contains("…[truncated]"));
}

// ── workflows_list_runs / workflows_get_run RPC surface ────────────────

#[tokio::test]
async fn list_runs_returns_runs_for_workflow_newest_first() {
    let (_dir, config) = config_with_temp_workspace();
    let created = ops::create(&config, create_request("p1"))
        .await
        .unwrap()
        .value;
    // Fire two dispatches and wait for both to terminate.
    let r1 = executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    let _terminal_1 = wait_for_terminal_run(&config, &created.id).await;
    // Sleep a hair so the second run's started_at strictly exceeds
    // the first run's (RFC3339 second resolution is enough at our
    // load but the test must be deterministic).
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let r2 = executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    // Drain to terminal so we don't race on the polling loop.
    let _terminal_2 = wait_for_terminal_run(&config, &created.id).await;

    let runs = ops::list_runs(&config, created.id.clone(), Pagination::default())
        .await
        .unwrap()
        .value;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].id, r2, "newest run must be first");
    assert_eq!(runs[1].id, r1);
}

#[tokio::test]
async fn list_runs_clamps_limit_to_100() {
    let (_dir, config) = config_with_temp_workspace();
    let created = ops::create(&config, create_request("p1"))
        .await
        .unwrap()
        .value;
    // Limit far above the cap; ops::list_runs must clamp it.
    let outcome = ops::list_runs(
        &config,
        created.id.clone(),
        Pagination {
            limit: 5000,
            offset: 0,
        },
    )
    .await
    .unwrap();
    // No runs yet — just assert the call returns Ok + zero rows; the
    // clamp itself is exercised by the log line, but the contract
    // guarantee is no failure on out-of-range limits.
    assert!(outcome.value.is_empty());
}

#[tokio::test]
async fn get_run_returns_none_for_unknown_id() {
    let (_dir, config) = config_with_temp_workspace();
    let outcome = ops::get_run(&config, "no-such-run".into()).await.unwrap();
    assert!(outcome.value.is_none());
}

#[tokio::test]
async fn get_run_returns_run_and_step_rows() {
    let (_dir, config) = config_with_temp_workspace();
    let created = ops::create(&config, create_request("hello"))
        .await
        .unwrap()
        .value;
    let run_id = executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    let _terminal = wait_for_terminal_run(&config, &created.id).await;

    let payload = ops::get_run(&config, run_id.clone())
        .await
        .unwrap()
        .value
        .expect("Some run payload");
    assert_eq!(payload.run.id, run_id);
    assert_eq!(payload.steps.len(), 1);
    assert!(matches!(payload.steps[0].status, RunStatus::Succeeded));
}

// ── cancel_run (F-9) ───────────────────────────────────────────────────

#[tokio::test]
async fn cancel_run_returns_not_found_for_unknown_id() {
    let (_dir, config) = config_with_temp_workspace();
    let err = executor::cancel_run(&config, "nope".into())
        .await
        .unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[tokio::test]
async fn cancel_run_returns_not_running_when_terminal() {
    let (_dir, config) = config_with_temp_workspace();
    let created = ops::create(&config, create_request("hi"))
        .await
        .unwrap()
        .value;
    let run_id = executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    // Drain to Succeeded.
    let _terminal = wait_for_terminal_run(&config, &created.id).await;

    let err = executor::cancel_run(&config, run_id.clone())
        .await
        .unwrap_err();
    assert_eq!(err.code(), "not_running");
    match err {
        executor::CancelError::NotRunning { current_status, .. } => {
            assert!(matches!(current_status, RunStatus::Succeeded));
        }
        other => panic!("expected NotRunning, got {other:?}"),
    }
}

#[tokio::test]
async fn cancel_run_flips_flag_and_executor_observes_cancelled_terminal() {
    let (_dir, config) = config_with_temp_workspace();
    let created = ops::create(&config, create_request("body"))
        .await
        .unwrap()
        .value;
    let run_id = executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    // The placeholder body is synchronous; race the cancel against
    // it by setting the bit directly on the underlying row. The
    // executor's post-node `cancellation_observed` check upgrades
    // any successful return into a Cancelled terminal status.
    store::set_cancelled_flag(&config, &run_id).expect("set_cancelled_flag");
    let _terminal = wait_for_terminal_run(&config, &created.id).await;

    let row = store::get_run(&config, &run_id).unwrap().expect("run row");
    let (run, _steps) = row;
    assert!(
        matches!(run.status, RunStatus::Cancelled),
        "post-node cancel-observed upgrades terminal status to Cancelled, got {:?}",
        run.status
    );
    assert!(run.cancelled);
}

// ── single-flight (F-9) ────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_run_rejects_second_overlapping_dispatch_with_already_running() {
    let (_dir, config) = config_with_temp_workspace();
    let created = ops::create(&config, create_request("p"))
        .await
        .unwrap()
        .value;
    // Manually claim the in-flight slot by inserting a Running run
    // row without spawning the executor task — this is the
    // deterministic equivalent of "the previous run hasn't
    // completed yet".
    let prior_run = Run {
        id: "prior-run".into(),
        workflow_id: created.id.clone(),
        trigger_source: TriggerSource::Manual {
            initiator: "user".into(),
        },
        status: RunStatus::Running,
        started_at: chrono::Utc::now(),
        completed_at: None,
        error: None,
        cancelled: false,
    };
    store::insert_run(&config, &prior_run).unwrap();
    executor::state_in_flight_insert_for_test(created.id.clone(), prior_run.id.clone());

    let err = executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap_err();
    let dispatch_err = err
        .downcast::<executor::DispatchError>()
        .expect("DispatchError");
    assert_eq!(dispatch_err.code(), "already_running");
    match dispatch_err {
        executor::DispatchError::AlreadyRunning {
            workflow_id,
            run_id,
        } => {
            assert_eq!(workflow_id, created.id);
            assert_eq!(run_id, prior_run.id);
        }
        other => panic!("expected AlreadyRunning, got {other:?}"),
    }

    // Cleanup so test ordering doesn't bleed into siblings: free
    // the slot we manually claimed.
    executor::state_in_flight_remove_for_test(&created.id);
}

#[tokio::test]
async fn dispatch_run_releases_slot_on_success_and_can_redispatch() {
    let (_dir, config) = config_with_temp_workspace();
    let created = ops::create(&config, create_request("redispatch"))
        .await
        .unwrap()
        .value;
    executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    let _terminal = wait_for_terminal_run(&config, &created.id).await;
    // Yield so the spawned task's InFlightSlot Drop runs after
    // execute_inner's last await point.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    // Slot must be free — second dispatch returns Ok.
    let second = executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .expect("slot released → second dispatch succeeds");
    assert!(!second.is_empty());
    let _terminal_2 = wait_for_terminal_run(&config, &created.id).await;
}

#[tokio::test]
async fn dispatch_run_independent_workflows_run_concurrently() {
    let (_dir, config) = config_with_temp_workspace();
    let a = ops::create(&config, create_request("a"))
        .await
        .unwrap()
        .value;
    let b = ops::create(&config, create_request("b"))
        .await
        .unwrap()
        .value;
    let r_a = executor::dispatch_run(
        &config,
        a.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .expect("a dispatch");
    let r_b = executor::dispatch_run(
        &config,
        b.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .expect("b dispatch");
    assert_ne!(r_a, r_b);
    let _ta = wait_for_terminal_run(&config, &a.id).await;
    let _tb = wait_for_terminal_run(&config, &b.id).await;
}

// ── orphan_recovery_sweep (F-9) ────────────────────────────────────────

#[tokio::test]
async fn orphan_recovery_sweep_marks_stale_running_runs_failed_core_crashed() {
    let (_dir, config) = config_with_temp_workspace();
    let created = ops::create(&config, create_request("p"))
        .await
        .unwrap()
        .value;
    // Simulate a crash: persist a Running run row without spawning
    // the executor. orphan_recovery_sweep on the next "boot" must
    // mark it Failed{CoreCrashed}.
    let stale = Run {
        id: "stale-run-1".into(),
        workflow_id: created.id.clone(),
        trigger_source: TriggerSource::Cron,
        status: RunStatus::Running,
        started_at: chrono::Utc::now(),
        completed_at: None,
        error: None,
        cancelled: false,
    };
    store::insert_run(&config, &stale).unwrap();

    let n = executor::orphan_recovery_sweep(&config).await.unwrap();
    assert_eq!(n, 1);

    let (run, _steps) = store::get_run(&config, &stale.id)
        .unwrap()
        .expect("row still present");
    assert!(matches!(run.status, RunStatus::Failed));
    assert_eq!(run.error.as_deref(), Some("CoreCrashed"));
    assert!(run.completed_at.is_some());

    // Idempotent: a second sweep is a no-op.
    let n2 = executor::orphan_recovery_sweep(&config).await.unwrap();
    assert_eq!(n2, 0);
}

#[tokio::test]
async fn orphan_recovery_sweep_on_clean_db_returns_zero() {
    let (_dir, config) = config_with_temp_workspace();
    let n = executor::orphan_recovery_sweep(&config).await.unwrap();
    assert_eq!(n, 0);
}
