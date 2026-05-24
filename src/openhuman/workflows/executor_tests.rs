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

/// FIFO queue of narratives the F-17 path of the unified stub returns.
/// Tests push one entry per planned `dispatch_run` call; the stub
/// pops in order. When empty the stub falls back to the legacy echo.
static NARRATIVE_SLOT: once_cell::sync::Lazy<
    parking_lot::Mutex<std::collections::VecDeque<String>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::VecDeque::new()));

/// Process-wide capture of every composed prompt seen by the unified
/// stub. F-17 integration tests inspect this to assert the recall
/// block landed in run 2's user-message preamble.
fn captured_prompts() -> &'static parking_lot::Mutex<Vec<String>> {
    use std::sync::OnceLock;
    static SLOT: OnceLock<parking_lot::Mutex<Vec<String>>> = OnceLock::new();
    SLOT.get_or_init(|| parking_lot::Mutex::new(Vec::new()))
}

/// Install the unified deterministic agent stub. Behavior:
///   - If the F-17 narrative queue ([`NARRATIVE_SLOT`]) has a pending
///     entry, pop it and return it as the agent's text. Used by the
///     F-17 integration tests to drive specific narratives + the
///     confabulation sentinel.
///   - Otherwise, fall back to the legacy echo behavior — `[test-stub]
///     prompt={} ({} chars), allowed_tools={}`. The F-8/F-9/F-16 tests
///     rely on this exact shape for their `output_json` assertions.
///
/// In both cases the prompt is captured into [`captured_prompts`] so
/// F-17 tests can inspect what the recall block looked like.
///
/// Idempotent: the slot is a `Mutex<Option<Fn>>`; calling this
/// repeatedly is fine — last-writer-wins is the desired contract.
fn install_test_agent_stub() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        set_test_agent_prompt_override(|prompt, def| {
            captured_prompts().lock().push(prompt.to_string());
            // Only F-17 tests embed the `F-17:` sentinel in their
            // workflow prompts. Non-F-17 tests running in parallel
            // must not pop entries off `NARRATIVE_SLOT` — otherwise
            // they'd steal the F-17 test's narratives and the F-17
            // tests would fall through to the echo path.
            if prompt.contains("F-17:") {
                if let Some(narrative) = NARRATIVE_SLOT.lock().pop_front() {
                    if let Some(rest) = narrative.strip_prefix("F17-FAIL-RUN:") {
                        let (run_id, real_narrative) =
                            rest.split_once('|').unwrap_or((rest, ""));
                        publish_synthetic_tool_failure(run_id.trim());
                        return Ok(real_narrative.to_string());
                    }
                    return Ok(narrative);
                }
            }
            // Legacy echo path — every pre-F-17 test depends on this
            // exact shape for their `[test-stub]` substring asserts.
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
    // Discovery tools (added when any Composio connection is present)
    // land right after the baseline names; the execute tool comes
    // after those; the Builtin-resolved tool follows. See the
    // module docstring on `build_node_agent_definition` for the
    // F-16-follow-up rationale (LLM needs discovery to find the
    // right action slug — otherwise it 400s with "Toolkit X is
    // not enabled" because it guesses the toolkit name as the slug).
    let baseline_len = BASELINE_TOOL_NAMES.len();
    assert_eq!(def.allowed_tools[baseline_len], "composio_list_toolkits");
    assert_eq!(def.allowed_tools[baseline_len + 1], "composio_list_tools");
    assert_eq!(def.allowed_tools[baseline_len + 2], "composio_execute");
    assert_eq!(def.allowed_tools[baseline_len + 3], "builtin_memory");
    assert_eq!(def.iteration_cap, 5);
    assert_eq!(def.model_tier.as_deref(), Some("reasoning"));
}

#[test]
fn build_node_agent_definition_omits_composio_discovery_tools_when_no_composio_connection() {
    // Non-Composio workflows must NOT carry the Composio discovery
    // surface — keeps the allowlist tight per ADR-016 ("nothing
    // else"). A Channel-only workflow's LLM has no business
    // listing Composio toolkits.
    let def = build_node_agent_definition(
        &[ConnectionRef::Channel {
            provider: "telegram".into(),
            channel_id: "@my_channel".into(),
        }],
        12,
        None,
    );
    assert!(
        !def.allowed_tools.iter().any(|t| t == "composio_list_tools"),
        "non-Composio workflow must not get composio_list_tools; allowlist: {:?}",
        def.allowed_tools
    );
    assert!(
        !def.allowed_tools
            .iter()
            .any(|t| t == "composio_list_toolkits"),
        "non-Composio workflow must not get composio_list_toolkits; allowlist: {:?}",
        def.allowed_tools
    );
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

// ─────────────────────────────────────────────────────────────
// F-16: honest step status via tool-failure event-bus tap
// ─────────────────────────────────────────────────────────────

/// Helper to publish a synthetic `ToolExecutionCompleted` event that
/// the F-16 tap inside `run_agent_prompt` will count toward
/// `tool_failure_count`. Used by tests that need to assert the
/// "step Failed because the agent called a denied tool" branch
/// without bringing up a real LLM provider.
fn publish_synthetic_tool_failure(run_id: &str) {
    use crate::core::event_bus::{publish_global, DomainEvent};
    publish_global(DomainEvent::ToolExecutionCompleted {
        tool_name: "composio_execute".into(),
        session_id: format!("workflow:{run_id}"),
        success: false,
        elapsed_ms: 42,
    });
}

/// F-16: ensure the executor's bus is initialised so the F-16 tap
/// can subscribe. The bus is a singleton; init_global is a no-op
/// when already initialised, so it's safe to call from every test.
fn ensure_event_bus_initialised() {
    use crate::core::event_bus::init_global;
    let _ = init_global(128);
}

/// F-16 — happy path with zero observed tool failures keeps the
/// existing `Succeeded` semantics. Existing 18 executor tests
/// already cover this implicitly (the test stub fires no events,
/// so `tool_failure_count` stays at 0); this test pins the
/// contract explicitly so an accidental change to "default Failed"
/// would surface here.
#[tokio::test]
async fn run_step_succeeded_when_zero_tool_failure_events_observed() {
    ensure_event_bus_initialised();
    let (_dir, config) = config_with_temp_workspace();
    let created = ops::create(&config, create_request("zero-failure path"))
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
    assert!(
        matches!(terminal.status, RunStatus::Succeeded),
        "no tool failures should yield Succeeded; got {:?}",
        terminal.status
    );
    let (_run, steps) = store::get_run(&config, &terminal.id)
        .unwrap()
        .expect("run row");
    assert!(matches!(steps[0].status, RunStatus::Succeeded));
    assert!(
        steps[0].error.is_none(),
        "Succeeded step must have no error summary"
    );
}

/// F-16 — when the harness reports any tool-call as `success=false`
/// during the run (denial or executed-with-error), the step status
/// is overridden to `Failed` with an honest summary, even though
/// the agent itself returned non-empty text.
///
/// We simulate the failure by publishing a synthetic
/// `ToolExecutionCompleted { success: false, session_id =
/// "workflow:<run_id>" }` from inside the test stub. The F-16
/// subscriber inside `run_agent_prompt` is bound to the same
/// `session_id`, so the counter increments and the caller forces
/// `RunStatus::Failed`.
///
/// This test pins the whole F-16 contract: "Succeeded means tools
/// fired; Failed means at least one didn't." It's the regression
/// guard that prevents the pre-F-16 lie ("the agent returned text,
/// therefore the workflow succeeded") from creeping back.
#[tokio::test]
async fn run_step_failed_when_tool_failure_event_observed_during_run() {
    ensure_event_bus_initialised();
    // The F-16 stub variant this test used to install was behaviourally
    // a no-op (its `strip_prefix("F-16-FAIL-RUN:")` branch never fired —
    // the workflow prompt below never carried that literal). It also
    // had a destructive side effect: it replaced the F-17 narrative-
    // pop stub that the F-17 integration tests rely on, causing
    // suite-mode F-17 failures. The unified stub installed by
    // `install_test_agent_stub` already echoes the prompt + observes
    // failures via the separate publish task spawned below, so no
    // per-test stub replacement is needed here.
    let (_dir, config) = config_with_temp_workspace();
    // Two-stage: create the workflow → dispatch → re-fetch the
    // run id → publish through the stub on next dispatch. Since
    // the executor materialises the run id BEFORE invoking the
    // agent, we can't predict it inside the prompt. Instead we
    // bake a sentinel into the prompt; the stub publishes against
    // the *current* run id by reading it back via the session_id
    // tag from the harness. But we don't have access to the
    // session_id inside the stub. So plan B: publish against
    // ALL `workflow:*` session ids — the bus tap inside
    // run_agent_prompt filters by exact match anyway. We hack
    // this by publishing using the wildcard signal "workflow:*"
    // which won't match; instead, dispatch the workflow first to
    // get the run_id, then publish AFTER the agent has started
    // but BEFORE it returns. Simpler: publish from a spawned
    // task tied to the dispatch.
    let created = ops::create(&config, create_request("F-16: failure-path workflow"))
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

    // Race the agent's `run_single`: publish the synthetic failure
    // tied to this exact run_id. The executor's tap holds a
    // SubscriptionHandle that was registered before the agent
    // started — broadcast::Receiver buffers everything from
    // subscription onward, so even if we publish slightly later
    // than the stub returns, the counter still observes it on the
    // next yield_now poll inside run_agent_prompt.
    //
    // To make this race deterministic, publish in a loop for up to
    // 500ms, terminating as soon as the run reaches a terminal
    // status — that way we cover the window even if the agent's
    // sync stub returns "instantly".
    let publish_run_id = run_id.clone();
    let publish_handle = tokio::spawn(async move {
        for _ in 0..50 {
            publish_synthetic_tool_failure(&publish_run_id);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    });

    let terminal = wait_for_terminal_run(&config, &created.id).await;
    publish_handle.abort();

    assert!(
        matches!(terminal.status, RunStatus::Failed),
        "F-16: a workflow run that observed a tool failure must end Failed; got {:?}",
        terminal.status
    );
    let (_run, steps) = store::get_run(&config, &terminal.id)
        .unwrap()
        .expect("run row");
    assert_eq!(steps.len(), 1);
    let step = &steps[0];
    assert!(matches!(step.status, RunStatus::Failed));
    let err = step
        .error
        .as_deref()
        .expect("F-16-failed step must carry an error summary");
    assert!(
        err.contains("tool call") && err.contains("reported as failed"),
        "error summary should reference tool failures; got: {err}"
    );
    // The agent's text is still persisted (so debugging the failed
    // run can see what the agent tried to say) — the status alone
    // changed.
    assert!(
        step.output_json.is_some(),
        "F-16: step text payload must be persisted even when status flips to Failed"
    );
}

/// F-16 — `build_node_agent_definition`'s output is the exact wire
/// the executor passes into `from_config_for_agent_with_tool_override`.
/// This is the contract test for the C deliverable.
#[test]
fn build_node_agent_definition_output_drives_workflow_node_allowlist() {
    // No connections: just baseline + read-only workflow tools. The
    // executor passes this entire list as the override into the
    // workflow_node archetype. If a future change accidentally adds
    // a `delegate_*` name here, the LLM would gain delegation
    // capability and the pre-F-16 bug would resurface — pin the
    // shape so it can't drift silently.
    let def = build_node_agent_definition(&[], 12, None);
    for name in &def.allowed_tools {
        assert!(
            !name.starts_with("delegate_"),
            "workflow_node allowlist must never contain a delegate_* tool name; found {name}"
        );
    }
    // Sanity: baseline tools are present at the head, read-only
    // workflow tools at the tail.
    assert_eq!(
        &def.allowed_tools[..BASELINE_TOOL_NAMES.len()],
        BASELINE_TOOL_NAMES
    );
    let tail_start = def.allowed_tools.len() - READ_ONLY_WORKFLOW_TOOL_NAMES.len();
    assert_eq!(
        &def.allowed_tools[tail_start..],
        READ_ONLY_WORKFLOW_TOOL_NAMES
    );
}

// ── F-17: memory loop integration tests ────────────────────────────────

/// Process-wide tempdir used to back the `memory::global` singleton
/// across F-17 integration tests. The first test to run wins (per
/// `OnceLock`); subsequent tests share the same workspace. Tests
/// isolate by using unique workflow ids so their `workflow:{id}`
/// namespaces never collide.
fn ensure_test_memory_global_initialised() -> String {
    use std::sync::OnceLock;
    static WORKSPACE: OnceLock<TempDir> = OnceLock::new();
    let ws = WORKSPACE.get_or_init(|| TempDir::new().expect("memory tempdir"));
    let _ = crate::openhuman::memory::global::init(ws.path().join("workspace"));
    ws.path().to_string_lossy().to_string()
}

/// Build a `Config` with a temp workspace AND prime the F-17 stub +
/// memory client global. Tests get a fresh local workspace for the
/// SQLite workflow DB, but share the process-wide memory workspace
/// (because `memory::global` is one-shot).
fn config_for_f17_test() -> (TempDir, Config) {
    install_test_agent_stub();
    ensure_event_bus_initialised();
    let _ws = ensure_test_memory_global_initialised();
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, config)
}

/// Reset F-17 test scaffolding: clear captured prompts + the
/// narrative queue. Tests call this on entry so test ordering
/// doesn't leak state.
fn reset_f17_scaffold() {
    captured_prompts().lock().clear();
    NARRATIVE_SLOT.lock().clear();
}

/// Serialise the F-17 integration tests — they share the agent stub's
/// narrative queue + the prompt-capture buffer, and `cargo test`
/// runs sibling tests in parallel by default. Holding this mutex
/// for the duration of each F-17 test guarantees the queue contains
/// only this test's narratives and the captures belong to this
/// test's runs.
///
/// Other (non-F-17) executor tests don't push to `NARRATIVE_SLOT`,
/// so they're free to keep running in parallel — the unified stub's
/// fallback path is stateless echo.
fn f17_test_lock() -> &'static parking_lot::Mutex<()> {
    use std::sync::OnceLock;
    static LOCK: OnceLock<parking_lot::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| parking_lot::Mutex::new(()))
}

/// Look up the stored `WorkflowRunMemory` chunk for a given workflow +
/// run id directly from the global memory client. Returns `None` when
/// the chunk is missing or unparseable.
async fn fetch_run_memory(
    workflow_id: &str,
    run_id: &str,
) -> Option<super::memory::WorkflowRunMemory> {
    use super::memory::{key_for_run, namespace_for, WorkflowRunMemory};
    let client = crate::openhuman::memory::global::client_if_ready()?;
    let mem = client.memory_handle();
    let entry = mem
        .get(&namespace_for(workflow_id), &key_for_run(run_id))
        .await
        .ok()??;
    WorkflowRunMemory::parse_storage_markdown(&entry.content)
}

#[tokio::test]
async fn memory_loop_first_run_renders_no_prior_runs_line() {
    let _guard = f17_test_lock().lock();
    reset_f17_scaffold();
    NARRATIVE_SLOT
        .lock()
        .push_back("Done — fetched 0 items.".to_string());

    let (_dir, config) = config_for_f17_test();
    // Unique workflow id keeps memory namespace isolated from other
    // tests sharing the global memory singleton.
    let mut req = create_request("F-17: first-run recall fallback");
    req.name = format!("f17-first-{}", uuid::Uuid::new_v4());
    let created = ops::create(&config, req).await.unwrap().value;

    executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    let _ = wait_for_terminal_run(&config, &created.id).await;

    // The stub captured every prompt it received; the first one for
    // this workflow_id must contain the "first execution" fallback
    // line.
    let prompts = captured_prompts().lock().clone();
    let our_prompt = prompts
        .iter()
        .find(|p| p.contains("F-17: first-run recall fallback"))
        .expect("captured prompt for our workflow");
    assert!(
        our_prompt.contains("## No prior runs — this is the first execution."),
        "first run must carry the no-prior-runs fallback line; got:\n{our_prompt}"
    );
}

#[tokio::test]
async fn memory_loop_stores_and_recalls_across_two_runs() {
    let _guard = f17_test_lock().lock();
    reset_f17_scaffold();
    // Two narratives — one per dispatch. The first becomes the prior-
    // run summary the second will see.
    NARRATIVE_SLOT
        .lock()
        .push_back("Sent the morning digest to #general.".to_string());
    NARRATIVE_SLOT
        .lock()
        .push_back("Second run — followed the prior pattern.".to_string());

    let (_dir, config) = config_for_f17_test();
    let mut req = create_request("F-17: two-run recall sequence");
    req.name = format!("f17-twin-{}", uuid::Uuid::new_v4());
    let created = ops::create(&config, req).await.unwrap().value;

    // Run 1.
    let run1_id = executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    let run1 = wait_for_terminal_run(&config, &created.id).await;
    assert!(matches!(run1.status, RunStatus::Succeeded));

    // Memory write must land before run 2's recall reads it.
    let stored = fetch_run_memory(&created.id, &run1_id)
        .await
        .expect("run 1 must persist a WorkflowRunMemory chunk");
    assert!(stored.narrative.contains("Sent the morning digest"));
    assert_eq!(stored.status, RunStatus::Succeeded);
    assert!(stored.narrative_matches_actual);
    assert!(stored.narrative_drift.is_empty());

    // Run 2 — its composed prompt must include run 1's recall line.
    captured_prompts().lock().clear();
    executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    let _ = wait_for_terminal_run(&config, &created.id).await;

    let prompts = captured_prompts().lock().clone();
    let run2_prompt = prompts
        .iter()
        .find(|p| p.contains("F-17: two-run recall sequence"))
        .expect("captured run-2 prompt");
    assert!(
        run2_prompt.contains("## Prior runs of this workflow"),
        "run 2's prompt must carry the recall header; got:\n{run2_prompt}"
    );
    assert!(
        !run2_prompt.contains("## No prior runs"),
        "run 2 must NOT see the first-execution fallback; got:\n{run2_prompt}"
    );
    // Run 1's narrative is included in the recall line per
    // `to_recall_line` happy-path branch.
    assert!(
        run2_prompt.contains("Sent the morning digest"),
        "run 2's recall must surface run 1's narrative; got:\n{run2_prompt}"
    );
}

#[tokio::test]
async fn memory_loop_confabulation_marks_failed_and_drifts_into_next_run() {
    let _guard = f17_test_lock().lock();
    reset_f17_scaffold();

    // Use a fresh config for the workflow_db (per-test).
    let (_dir, config) = config_for_f17_test();
    let mut req = create_request("F-17: confabulation regression");
    req.name = format!("f17-lie-{}", uuid::Uuid::new_v4());
    let created = ops::create(&config, req).await.unwrap().value;

    // We need run 1's id to drive the synthetic failure event. Stub
    // returns a "lying" narrative ("Sent the digest!") AND publishes
    // a failure event keyed by the run id. The run id isn't known
    // until dispatch returns — so we use a two-phase approach: get
    // the run id, then race the synthetic failure publish (mirroring
    // the F-16 test pattern).
    //
    // First, set up narrative for run 1 — plain "Sent the digest!"
    // claim. The publish is driven externally via the same publish
    // helper the F-16 test uses.
    NARRATIVE_SLOT
        .lock()
        .push_back("Sent the digest to #general. All set.".to_string());

    let run1_id = executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();

    // Race the agent: publish a synthetic failure event tied to run 1
    // for up to 500ms or until the run terminates.
    let publish_run_id = run1_id.clone();
    let publish_handle = tokio::spawn(async move {
        for _ in 0..50 {
            publish_synthetic_tool_failure(&publish_run_id);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    });

    let run1 = wait_for_terminal_run(&config, &created.id).await;
    publish_handle.abort();

    // Honest-status gate must override Succeeded → Failed.
    assert!(
        matches!(run1.status, RunStatus::Failed),
        "confabulation: a run with observed tool failures must end Failed; got {:?}",
        run1.status
    );

    // Stored memory must reflect the drift.
    let stored = fetch_run_memory(&created.id, &run1_id)
        .await
        .expect("run 1 must persist a WorkflowRunMemory chunk even on failure");
    assert_eq!(stored.status, RunStatus::Failed);
    assert!(
        !stored.narrative_matches_actual,
        "narrative claimed 'sent' but trace shows failure — must record drift"
    );
    assert!(
        !stored.narrative_drift.is_empty(),
        "drift entries must surface in stored memory; got: {stored:?}"
    );

    // Run 2: its recall block must carry the ⚠ annotation so the
    // next-run agent doesn't inherit the confabulation.
    captured_prompts().lock().clear();
    NARRATIVE_SLOT
        .lock()
        .push_back("Run 2 — saw the drift annotation.".to_string());
    executor::dispatch_run(
        &config,
        created.id.clone(),
        TriggerSource::Manual {
            initiator: "user".into(),
        },
    )
    .await
    .unwrap();
    let _ = wait_for_terminal_run(&config, &created.id).await;

    let prompts = captured_prompts().lock().clone();
    let run2_prompt = prompts
        .iter()
        .find(|p| p.contains("F-17: confabulation regression"))
        .expect("captured run-2 prompt");
    assert!(
        run2_prompt.contains("⚠ Narrative drift:"),
        "next run must see the drift warning; got:\n{run2_prompt}"
    );
    assert!(
        run2_prompt.contains("DO NOT assume the narrative is true"),
        "drift annotation must include the safety guidance; got:\n{run2_prompt}"
    );
}
