//! F-10 — round-trip tests for the four read-only workflow tools.
//!
//! Asserts each tool against the same `ops::*` path the matching RPC
//! uses, so a single fixture covers both surfaces. The allowlist
//! enforcement test lives alongside (rather than in
//! `workflows::agent_tools_tests`) so a refactor of the tool-impl
//! crate doesn't drift the assertion location.

use super::*;
use crate::openhuman::config::Config;
use crate::openhuman::connections::types::ConnectionRef;
use crate::openhuman::tools::traits::Tool;
use crate::openhuman::workflows::executor::{
    build_node_agent_definition, BASELINE_TOOL_NAMES, READ_ONLY_WORKFLOW_TOOL_NAMES,
};
use crate::openhuman::workflows::ops;
use crate::openhuman::workflows::types::{
    AgentPromptConfig, CreateWorkflowRequest, Node, NodeConfig, NodeKind, Trigger, WorkflowOrigin,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tempfile::TempDir;

fn config_with_temp_workspace() -> (TempDir, Arc<Config>) {
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, Arc::new(config))
}

fn sample_create(prompt: &str) -> CreateWorkflowRequest {
    CreateWorkflowRequest {
        name: "agent-tool-test".into(),
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

fn parse_output(out: &str) -> Value {
    serde_json::from_str(out).unwrap_or_else(|_| panic!("tool output not JSON: {out}"))
}

// ── Tool name + permission contract ────────────────────────────────────

#[test]
fn tool_names_match_canonical_constants() {
    let (_d, config) = config_with_temp_workspace();
    let list = WorkflowListTool::new(config.clone());
    let get = WorkflowGetTool::new(config.clone());
    let list_runs = WorkflowsListRunsTool::new(config.clone());
    let get_run = WorkflowsGetRunTool::new(config.clone());
    assert_eq!(list.name(), TOOL_WORKFLOW_LIST);
    assert_eq!(get.name(), TOOL_WORKFLOW_GET);
    assert_eq!(list_runs.name(), TOOL_WORKFLOWS_LIST_RUNS);
    assert_eq!(get_run.name(), TOOL_WORKFLOWS_GET_RUN);
}

#[test]
fn read_only_tool_names_constant_matches_executor_constant() {
    // F-8 hard-coded the four names inside the executor's allowlist
    // builder. F-10's tools are the canonical source. The two must
    // stay in lock-step — drift produces a runtime "tool not found"
    // when an `agent_prompt` node tries to resolve a stale name.
    let mut a: Vec<&str> = READ_ONLY_TOOL_NAMES.to_vec();
    let mut b: Vec<&str> = READ_ONLY_WORKFLOW_TOOL_NAMES.to_vec();
    a.sort();
    b.sort();
    assert_eq!(a, b);
}

// ── Round-trip ─────────────────────────────────────────────────────────

#[tokio::test]
async fn workflow_list_round_trips_against_ops() {
    let (_d, config) = config_with_temp_workspace();
    let created = ops::create(&config, sample_create("p"))
        .await
        .unwrap()
        .value;

    let tool = WorkflowListTool::new(config.clone());
    let result = tool.execute(json!({})).await.unwrap();
    assert!(!result.is_error);
    let payload = parse_output(&result.output());
    let workflows = payload["workflows"].as_array().expect("workflows array");
    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0]["id"].as_str(), Some(created.id.as_str()));
}

#[tokio::test]
async fn workflow_get_round_trips_against_ops() {
    let (_d, config) = config_with_temp_workspace();
    let created = ops::create(&config, sample_create("p"))
        .await
        .unwrap()
        .value;

    let tool = WorkflowGetTool::new(config.clone());
    let result = tool
        .execute(json!({ "workflow_id": created.id }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let payload = parse_output(&result.output());
    assert_eq!(
        payload["workflow"]["id"].as_str(),
        Some(created.id.as_str())
    );
}

#[tokio::test]
async fn workflow_get_returns_null_for_unknown_id() {
    let (_d, config) = config_with_temp_workspace();
    let tool = WorkflowGetTool::new(config);
    let result = tool
        .execute(json!({ "workflow_id": "no-such-id" }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let payload = parse_output(&result.output());
    assert!(payload["workflow"].is_null());
}

#[tokio::test]
async fn workflows_list_runs_round_trips_with_pagination_bounds() {
    let (_d, config) = config_with_temp_workspace();
    let created = ops::create(&config, sample_create("p"))
        .await
        .unwrap()
        .value;

    let tool = WorkflowsListRunsTool::new(config);
    // Out-of-range limit must be clamped by ops::list_runs, not error.
    let result = tool
        .execute(json!({ "workflow_id": created.id, "limit": 99999 }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let payload = parse_output(&result.output());
    let runs = payload["runs"].as_array().expect("runs array");
    assert!(runs.is_empty(), "fresh workflow has no runs");
}

#[tokio::test]
async fn workflows_get_run_returns_null_when_run_id_unknown() {
    let (_d, config) = config_with_temp_workspace();
    let tool = WorkflowsGetRunTool::new(config);
    let result = tool.execute(json!({ "run_id": "ghost" })).await.unwrap();
    assert!(!result.is_error);
    let payload = parse_output(&result.output());
    assert!(payload["run_with_steps"].is_null());
}

// ── Secret-leak regression ─────────────────────────────────────────────

#[tokio::test]
async fn workflow_get_does_not_leak_secret_ref_through_generic_http_connection() {
    // A workflow's `agent_prompt` node carries a `ConnectionRef::GenericHttp { connection_id }`
    // — credentials live in `connections.db` and never transit
    // through the workflow JSON. Assert structurally: neither the
    // literal "secret_ref" field nor any plausible secret payload
    // appears in the serialised workflow row.
    let (_d, config) = config_with_temp_workspace();
    let req = CreateWorkflowRequest {
        name: "leak-check".into(),
        description: None,
        trigger: Trigger::Manual,
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "x".into(),
                allowed_connections: vec![ConnectionRef::GenericHttp {
                    connection_id: "http-conn-id".into(),
                }],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
        }],
        edges: vec![],
        settings: None,
        origin: WorkflowOrigin::UserChat,
    };
    let created = ops::create(&config, req).await.unwrap().value;

    let tool = WorkflowGetTool::new(config.clone());
    let result = tool
        .execute(json!({ "workflow_id": created.id }))
        .await
        .unwrap();
    let body = result.output();
    assert!(
        !body.contains("secret_ref"),
        "workflow_get output leaked literal `secret_ref`: {body}"
    );
    assert!(
        !body.contains("Bearer ") && !body.contains("Basic "),
        "workflow_get output leaked an Authorization-header secret: {body}"
    );
}

// ── Allowlist enforcement (NFR-2.3.7) ──────────────────────────────────

#[test]
fn build_node_agent_definition_matches_baseline_plus_read_only_with_no_connections() {
    let def = build_node_agent_definition(&[], 12, None);
    for b in BASELINE_TOOL_NAMES {
        assert!(
            def.allowed_tools.iter().any(|n| n == b),
            "missing baseline tool: {b}"
        );
    }
    for t in READ_ONLY_TOOL_NAMES {
        assert!(
            def.allowed_tools.iter().any(|n| n == t),
            "missing read-only workflow tool: {t}"
        );
    }
    // Negative: zero propose tools, zero mutations.
    let propose_count = def
        .allowed_tools
        .iter()
        .filter(|n| n.starts_with("workflow_propose_"))
        .count();
    assert_eq!(
        propose_count, 0,
        "no propose tools should be in F-10 allowlist"
    );
    for forbidden in crate::openhuman::workflows::agent_tools::FORBIDDEN_MUTATING_TOOL_NAMES {
        assert!(
            !def.allowed_tools.iter().any(|n| n == forbidden),
            "mutating name leaked into allowlist: {forbidden}"
        );
    }
}

#[test]
fn build_node_agent_definition_includes_connection_resolved_names_plus_read_only() {
    let allowed = vec![ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    }];
    let def = build_node_agent_definition(&allowed, 12, None);
    assert!(def.allowed_tools.iter().any(|n| n == "composio_execute"));
    for t in READ_ONLY_TOOL_NAMES {
        assert!(def.allowed_tools.iter().any(|n| n == t));
    }
    for forbidden in crate::openhuman::workflows::agent_tools::FORBIDDEN_MUTATING_TOOL_NAMES {
        assert!(
            !def.allowed_tools.iter().any(|n| n == forbidden),
            "mutating name leaked into allowlist: {forbidden}"
        );
    }
}

#[test]
fn registered_tools_contain_no_mutating_workflow_names() {
    // Walks the full tool list that `tools::ops::all_tools_with_runtime`
    // hands the agent harness; asserts no entry matches the
    // FORBIDDEN list. This is the load-bearing security boundary
    // from ADR-012 — drift here means an agent could mutate.
    use crate::openhuman::config::{BrowserConfig, HttpRequestConfig, MemoryConfig};
    use crate::openhuman::memory::Memory;
    use crate::openhuman::security::SecurityPolicy;
    use std::collections::HashMap;

    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().join("workspace");
    config.config_path = dir.path().join("config.toml");
    let config = Arc::new(config);
    let security = Arc::new(SecurityPolicy::default());
    let mem_cfg = MemoryConfig {
        backend: "markdown".into(),
        ..MemoryConfig::default()
    };
    let memory: Arc<dyn Memory> =
        Arc::from(crate::openhuman::memory::create_memory(&mem_cfg, dir.path()).unwrap());
    let browser = BrowserConfig::default();
    let http = HttpRequestConfig::default();
    let agents = HashMap::new();

    let tools = crate::openhuman::tools::all_tools(
        config.clone(),
        &security,
        memory,
        &browser,
        &http,
        &config.workspace_dir,
        &agents,
        &config,
    );
    let names: Vec<String> = tools.iter().map(|t| t.name().to_string()).collect();

    for forbidden in crate::openhuman::workflows::agent_tools::FORBIDDEN_MUTATING_TOOL_NAMES {
        assert!(
            !names.iter().any(|n| n == forbidden),
            "mutating tool name registered on agent surface: {forbidden} (registered = {names:?})"
        );
    }

    // Positive: all four read-only + six propose-only workflow
    // tools are present after F-12.
    for expected in PROPOSE_TOOL_NAMES {
        assert!(
            names.iter().any(|n| n == expected),
            "F-12 propose tool not registered: {expected} (registered = {names:?})"
        );
    }
    for expected in READ_ONLY_TOOL_NAMES {
        assert!(
            names.iter().any(|n| n == expected),
            "F-10 tool not registered: {expected} (registered = {names:?})"
        );
    }
}

// ── F-12 propose tools ─────────────────────────────────────────────────

#[test]
fn propose_tool_names_constant_carries_all_six_propose_tools() {
    let mut names: Vec<&str> = PROPOSE_TOOL_NAMES.to_vec();
    names.sort();
    let mut expected = vec![
        TOOL_WORKFLOW_PROPOSE_CREATE,
        TOOL_WORKFLOW_PROPOSE_UPDATE,
        TOOL_WORKFLOW_PROPOSE_DELETE,
        TOOL_WORKFLOW_PROPOSE_ENABLE,
        TOOL_WORKFLOW_PROPOSE_DISABLE,
        TOOL_WORKFLOW_PROPOSE_RUN_NOW,
    ];
    expected.sort();
    assert_eq!(names, expected);
}

#[test]
fn build_node_agent_definition_excludes_propose_tools() {
    // F-12 regression check (ADR-016): propose tools must NOT leak
    // into the `agent_prompt` sub-agent's allowlist. They live on
    // the chat-agent surface only.
    let def = build_node_agent_definition(&[], 12, None);
    for propose in PROPOSE_TOOL_NAMES {
        assert!(
            !def.allowed_tools.iter().any(|n| n == propose),
            "F-12 propose tool leaked into agent_prompt allowlist: {propose}"
        );
    }
    // Also belt-and-suspenders: the connection-resolved variant.
    let def = build_node_agent_definition(
        &[ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        }],
        12,
        None,
    );
    for propose in PROPOSE_TOOL_NAMES {
        assert!(
            !def.allowed_tools.iter().any(|n| n == propose),
            "F-12 propose tool leaked into agent_prompt allowlist (with connections): {propose}"
        );
    }
}

#[tokio::test]
async fn workflow_propose_enable_returns_state_proposal_enable_payload() {
    let (_d, config) = config_with_temp_workspace();
    let req = CreateWorkflowRequest {
        name: "for-enable".into(),
        description: None,
        trigger: Trigger::Cron {
            expr: "0 9 * * 1-5".into(),
            tz: None,
            active_hours: None,
        },
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
        }],
        edges: vec![],
        settings: None,
        origin: WorkflowOrigin::UserChat,
    };
    let created = ops::create(&config, req).await.unwrap().value;

    let tool = WorkflowProposeEnableTool::new(config.clone());
    let result = tool
        .execute(json!({ "workflow_id": created.id }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let payload = parse_output(&result.output());
    assert_eq!(payload["state_proposal"]["action"].as_str(), Some("enable"));
    let rationale = payload["state_proposal"]["rationale"]
        .as_array()
        .expect("rationale array");
    assert!(!rationale.is_empty());
    assert!(rationale[0].as_str().unwrap().contains("0 9 * * 1-5"));
}

#[tokio::test]
async fn workflow_propose_disable_returns_state_proposal_disable_payload() {
    let (_d, config) = config_with_temp_workspace();
    let created = ops::create(&config, sample_create("p"))
        .await
        .unwrap()
        .value;
    let tool = WorkflowProposeDisableTool::new(config.clone());
    let result = tool
        .execute(json!({ "workflow_id": created.id }))
        .await
        .unwrap();
    let payload = parse_output(&result.output());
    assert_eq!(
        payload["state_proposal"]["action"].as_str(),
        Some("disable")
    );
    // Workflow defaults `enabled: false` — rationale notes the no-op.
    let rationale = payload["state_proposal"]["rationale"]
        .as_array()
        .expect("rationale array");
    assert!(rationale[0].as_str().unwrap().contains("Already disabled"));
}

#[tokio::test]
async fn workflow_propose_delete_returns_preview_with_run_count_and_retention_30() {
    let (_d, config) = config_with_temp_workspace();
    let created = ops::create(&config, sample_create("p"))
        .await
        .unwrap()
        .value;
    let tool = WorkflowProposeDeleteTool::new(config.clone());
    let result = tool
        .execute(json!({ "workflow_id": created.id }))
        .await
        .unwrap();
    let payload = parse_output(&result.output());
    let preview = &payload["delete_preview"];
    assert_eq!(preview["workflow_id"].as_str(), Some(created.id.as_str()));
    assert_eq!(preview["name"].as_str(), Some("agent-tool-test"));
    assert_eq!(preview["run_count"].as_u64(), Some(0));
    assert_eq!(preview["retention_days"].as_u64(), Some(30));
}

#[tokio::test]
async fn workflow_propose_run_now_returns_run_now_when_health_is_ready() {
    let (_d, config) = config_with_temp_workspace();
    let created = ops::create(&config, sample_create("p"))
        .await
        .unwrap()
        .value;
    assert!(matches!(
        created.health,
        crate::openhuman::workflows::types::WorkflowHealth::Ready
    ));
    let tool = WorkflowProposeRunNowTool::new(config.clone());
    let result = tool
        .execute(json!({ "workflow_id": created.id }))
        .await
        .unwrap();
    let payload = parse_output(&result.output());
    assert_eq!(
        payload["state_proposal"]["action"].as_str(),
        Some("run_now")
    );
    assert_eq!(payload["state_proposal"]["enabled"].as_bool(), Some(true));
    let rationale = payload["state_proposal"]["rationale"]
        .as_array()
        .expect("rationale array");
    assert!(rationale[0].as_str().unwrap().contains("Estimated time"));
}

#[tokio::test]
async fn workflow_propose_run_now_returns_disabled_when_health_blocks() {
    // Create a workflow whose `allowed_connections` references a
    // connection not in the snapshot — health is NeedsConnections.
    let (_d, config) = config_with_temp_workspace();
    let req = CreateWorkflowRequest {
        name: "blocked".into(),
        description: None,
        trigger: Trigger::Manual,
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "x".into(),
                allowed_connections: vec![ConnectionRef::Composio {
                    toolkit_id: "missing-toolkit".into(),
                    account_id: None,
                }],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
        }],
        edges: vec![],
        settings: None,
        origin: WorkflowOrigin::UserChat,
    };
    let created = ops::create(&config, req).await.unwrap().value;
    assert!(matches!(
        created.health,
        crate::openhuman::workflows::types::WorkflowHealth::NeedsConnections { .. }
    ));

    let tool = WorkflowProposeRunNowTool::new(config.clone());
    let result = tool
        .execute(json!({ "workflow_id": created.id }))
        .await
        .unwrap();
    let payload = parse_output(&result.output());
    assert_eq!(payload["state_proposal"]["enabled"].as_bool(), Some(false));
    let rationale = payload["state_proposal"]["rationale"].as_array().unwrap();
    assert!(rationale[0].as_str().unwrap().contains("Cannot run"));
}

#[tokio::test]
async fn workflow_propose_create_returns_drafting_failed_with_f11_placeholder() {
    // The AgentDrafter is the F-11 placeholder. Calling
    // workflow_propose_create surfaces its RunFailure as a
    // structured `{ error: "drafting_failed", ... }` payload.
    let (_d, config) = config_with_temp_workspace();
    let tool = WorkflowProposeCreateTool::new(config.clone());
    let result = tool
        .execute(json!({ "description": "build me a digest" }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let payload = parse_output(&result.output());
    assert_eq!(payload["error"].as_str(), Some("drafting_failed"));
    assert_eq!(payload["kind_label"].as_str(), Some("run_failure"));
    assert!(payload["reason"]
        .as_str()
        .unwrap()
        .contains("F-11 placeholder"));
}

#[tokio::test]
async fn workflow_propose_create_rejects_empty_description() {
    let (_d, config) = config_with_temp_workspace();
    let tool = WorkflowProposeCreateTool::new(config);
    let result = tool.execute(json!({ "description": "   " })).await.unwrap();
    let payload = parse_output(&result.output());
    assert_eq!(payload["error"].as_str(), Some("empty_description"));
}

#[tokio::test]
async fn workflow_propose_update_returns_not_found_for_unknown_workflow_id() {
    let (_d, config) = config_with_temp_workspace();
    let tool = WorkflowProposeUpdateTool::new(config);
    let result = tool
        .execute(json!({
            "workflow_id": "ghost",
            "instructions": "make it run at 9"
        }))
        .await
        .unwrap();
    let payload = parse_output(&result.output());
    assert_eq!(payload["error"].as_str(), Some("not_found"));
}

#[tokio::test]
async fn propose_tools_appear_alongside_read_only_in_registered_set() {
    use crate::openhuman::config::{BrowserConfig, HttpRequestConfig, MemoryConfig};
    use crate::openhuman::memory::Memory;
    use crate::openhuman::security::SecurityPolicy;
    use std::collections::HashMap;

    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().join("workspace");
    config.config_path = dir.path().join("config.toml");
    let config = Arc::new(config);
    let security = Arc::new(SecurityPolicy::default());
    let mem_cfg = MemoryConfig {
        backend: "markdown".into(),
        ..MemoryConfig::default()
    };
    let memory: Arc<dyn Memory> =
        Arc::from(crate::openhuman::memory::create_memory(&mem_cfg, dir.path()).unwrap());
    let browser = BrowserConfig::default();
    let http = HttpRequestConfig::default();
    let agents = HashMap::new();

    let tools = crate::openhuman::tools::all_tools(
        config.clone(),
        &security,
        memory,
        &browser,
        &http,
        &config.workspace_dir,
        &agents,
        &config,
    );
    let names: Vec<String> = tools.iter().map(|t| t.name().to_string()).collect();

    // The full 10 — 4 read + 6 propose.
    for expected in READ_ONLY_TOOL_NAMES.iter().chain(PROPOSE_TOOL_NAMES.iter()) {
        assert!(
            names.iter().any(|n| n == expected),
            "F-10/F-12 tool not registered: {expected}"
        );
    }
}
