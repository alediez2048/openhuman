//! F-11 — drafting retry-loop tests against a deterministic
//! `MockDrafter`. Scenarios per NFR-2.6.6: success-on-attempt-1,
//! fail-then-succeed-attempt-3, fail-all-attempts.

use super::proposer::{
    build_system_prompt, draft_with_retries, AgentDrafter, Drafter, RunFailure,
    DEFAULT_ITERATION_CAP, DEFAULT_MAX_ATTEMPTS, DRAFTING_TOOL_ALLOWLIST,
};
use super::types::*;
use crate::openhuman::connections::types::{ConnectionRef, ConnectionStatus, ConnectionView};
use crate::openhuman::workflows::health::ConnectionsSnapshot;
use async_trait::async_trait;
use std::sync::Mutex;

// ── MockDrafter ────────────────────────────────────────────────────────

/// Scripts the drafting sub-agent's response per attempt. Each
/// `responses[i]` is either:
///   - `Ok(proposal)` — the sub-agent emitted this proposal on
///     attempt `i + 1`.
///   - `Err(RunFailure)` — the sub-agent failed without emitting.
///
/// If the loop hits `responses[responses.len()]`, the mock panics —
/// every test should pin the exact number of attempts it expects.
struct MockDrafter {
    responses: Mutex<Vec<Result<WorkflowProposal, RunFailure>>>,
    call_count: Mutex<u32>,
}

impl MockDrafter {
    fn new(responses: Vec<Result<WorkflowProposal, RunFailure>>) -> Self {
        Self {
            responses: Mutex::new(responses),
            call_count: Mutex::new(0),
        }
    }

    fn calls(&self) -> u32 {
        *self.call_count.lock().unwrap()
    }
}

#[async_trait]
impl Drafter for MockDrafter {
    async fn draft(
        &self,
        _system_prompt: &str,
        _description: &str,
    ) -> Result<WorkflowProposal, RunFailure> {
        let mut count = self.call_count.lock().unwrap();
        *count += 1;
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            panic!(
                "MockDrafter ran out of scripted responses on call {}",
                *count
            );
        }
        responses.remove(0)
    }
}

// ── Fixtures ───────────────────────────────────────────────────────────

fn valid_proposal() -> WorkflowProposal {
    WorkflowProposal {
        name: "Morning digest".into(),
        description: "Send me a 7am summary".into(),
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
        settings: WorkflowSettings::default(),
        required_connections: vec![],
        rationale: vec!["because reasons".into()],
        confidence: Confidence::High,
    }
}

fn proposal_missing_name() -> WorkflowProposal {
    let mut p = valid_proposal();
    p.name = String::new();
    p
}

fn composio_view(toolkit: &str) -> ConnectionView {
    ConnectionView {
        r#ref: ConnectionRef::Composio {
            toolkit_id: toolkit.into(),
            account_id: None,
        },
        display_name: toolkit.into(),
        status: ConnectionStatus::Connected,
        last_used_at: None,
        mechanism_label: "Composio".into(),
        verification: None,
    }
}

// ── Retry-loop scenarios (NFR-2.6.6) ───────────────────────────────────

#[tokio::test]
async fn draft_with_retries_returns_proposal_on_attempt_1_success() {
    let drafter = MockDrafter::new(vec![Ok(valid_proposal())]);
    let snapshot = ConnectionsSnapshot::empty();
    let result = draft_with_retries(&drafter, "build me a digest", &snapshot, 1, 3).await;
    let proposal = result.expect("attempt-1 success should return Ok");
    assert_eq!(proposal.name, "Morning digest");
    assert_eq!(drafter.calls(), 1, "drafter should be called exactly once");
}

#[tokio::test]
async fn draft_with_retries_succeeds_on_third_attempt_after_two_validation_failures() {
    let drafter = MockDrafter::new(vec![
        Ok(proposal_missing_name()), // attempt 1 → missing_required_field
        Ok(proposal_missing_name()), // attempt 2 → missing_required_field
        Ok(valid_proposal()),        // attempt 3 → Ok
    ]);
    let snapshot = ConnectionsSnapshot::empty();
    let result = draft_with_retries(&drafter, "x", &snapshot, 1, 3).await;
    assert!(
        result.is_ok(),
        "attempt-3 success should return Ok, got {result:?}"
    );
    assert_eq!(drafter.calls(), 3, "drafter must be called 3 times");
}

#[tokio::test]
async fn draft_with_retries_returns_validation_failed_after_three_attempts() {
    let drafter = MockDrafter::new(vec![
        Ok(proposal_missing_name()),
        Ok(proposal_missing_name()),
        Ok(proposal_missing_name()),
    ]);
    let snapshot = ConnectionsSnapshot::empty();
    let err = draft_with_retries(&drafter, "x", &snapshot, 1, 3)
        .await
        .expect_err("all-fail must return Err");
    assert_eq!(err.kind_label(), "validation_failed_after_retries");
    match err {
        DraftFailure::ValidationFailedAfterRetries {
            attempts,
            last_error,
        } => {
            assert_eq!(attempts, 3);
            assert_eq!(last_error.kind_label(), "missing_required_field");
        }
        other => panic!("expected ValidationFailedAfterRetries, got {other:?}"),
    }
    assert_eq!(drafter.calls(), 3);
}

#[tokio::test]
async fn draft_with_retries_propagates_run_failure_without_retrying() {
    // A sub-agent RunFailure is orthogonal to validation — the
    // retry loop must not consume retries on transient failures.
    let drafter = MockDrafter::new(vec![
        Err(RunFailure::new("LLM provider 503")),
        // Even though more responses are queued, the call count
        // should stop at 1.
        Ok(valid_proposal()),
    ]);
    let snapshot = ConnectionsSnapshot::empty();
    let err = draft_with_retries(&drafter, "x", &snapshot, 1, 3)
        .await
        .expect_err("RunFailure must surface");
    assert_eq!(err.kind_label(), "run_failure");
    assert_eq!(drafter.calls(), 1, "RunFailure must not consume retries");
}

#[tokio::test]
async fn draft_with_retries_rejects_zero_attempts() {
    let drafter = MockDrafter::new(vec![]);
    let snapshot = ConnectionsSnapshot::empty();
    let err = draft_with_retries(&drafter, "x", &snapshot, 1, 0)
        .await
        .expect_err("max_attempts = 0 must error");
    assert_eq!(err.kind_label(), "run_failure");
}

// ── build_system_prompt ────────────────────────────────────────────────

#[test]
fn build_system_prompt_includes_connections_summary_on_empty_snapshot() {
    let snapshot = ConnectionsSnapshot::empty();
    let prompt = build_system_prompt(&snapshot, 1, None);
    assert!(prompt.contains("Your connections"));
    assert!(prompt.contains("no connections yet"));
    assert!(prompt.contains("Phase 1"));
    assert!(prompt.contains("AgentPrompt"));
    // Empty snapshot, no last_error → no PREVIOUS ATTEMPT block.
    assert!(!prompt.contains("PREVIOUS ATTEMPT FAILED"));
}

#[test]
fn build_system_prompt_groups_connections_by_mechanism() {
    let snapshot = ConnectionsSnapshot::new(vec![composio_view("gmail"), composio_view("slack")]);
    let prompt = build_system_prompt(&snapshot, 1, None);
    assert!(prompt.contains("**Composio**"));
    assert!(prompt.contains("gmail"));
    assert!(prompt.contains("slack"));
}

#[test]
fn build_system_prompt_appends_previous_attempt_failed_block_when_last_error_present() {
    let snapshot = ConnectionsSnapshot::empty();
    let last_error = ProposalValidationError::UnknownConnection {
        r#ref: ConnectionRef::Composio {
            toolkit_id: "gmaill".into(),
            account_id: None,
        },
        candidates: vec![ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        }],
    };
    let prompt = build_system_prompt(&snapshot, 1, Some(&last_error));
    assert!(prompt.contains("PREVIOUS ATTEMPT FAILED"));
    assert!(prompt.contains("unknown_connection"));
    assert!(prompt.contains("gmaill"));
    assert!(prompt.contains("gmail"));
}

#[test]
fn build_system_prompt_phase_block_lists_phase_2_kinds_when_phase_is_2() {
    let snapshot = ConnectionsSnapshot::empty();
    let p1 = build_system_prompt(&snapshot, 1, None);
    let p2 = build_system_prompt(&snapshot, 2, None);
    assert!(p1.contains("Phase 1"));
    assert!(p2.contains("Phase 2"));
    assert!(!p1.contains("HttpRequest"));
    assert!(p2.contains("HttpRequest"));
}

// ── Allowlist + constants contract ─────────────────────────────────────

#[test]
fn drafting_tool_allowlist_matches_adr_016() {
    // ADR-016 § "Drafting sub-agent allowlist":
    // [list_connections, workflow_list, emit_proposal] — and nothing
    // else. Adding here requires updating ADR-016 + F-12 in lock-step.
    assert_eq!(
        DRAFTING_TOOL_ALLOWLIST,
        &["list_connections", "workflow_list", "emit_proposal"]
    );
}

#[test]
fn default_max_attempts_matches_adr_015() {
    assert_eq!(DEFAULT_MAX_ATTEMPTS, 3);
}

#[test]
fn default_iteration_cap_matches_fr_1_13_2() {
    assert_eq!(DEFAULT_ITERATION_CAP, 6);
}

// ── Output-format instruction (F-15 drafter swap) ──────────────────────

#[test]
fn build_system_prompt_includes_output_format_override_instruction() {
    // The swap from `emit_proposal` (synthetic tool) to fenced ```json
    // depends on the OUTPUT FORMAT override being present in every
    // composed prompt. A test guards the contract.
    let snapshot = ConnectionsSnapshot::empty();
    let prompt = super::proposer::build_system_prompt(&snapshot, 1, None);
    assert!(prompt.contains("CRITICAL OVERRIDE"));
    assert!(prompt.contains("fenced JSON code block"));
    assert!(prompt.contains("`WorkflowProposal`"));
}

// ── F-13 bundling smoke tests ──────────────────────────────────────────

#[test]
fn bundled_workflow_builder_prompt_carries_canonical_signature() {
    // If the build picked up an empty / wrong file, the signature
    // line from the artifact won't be present and the build is
    // broken in a way the runtime would not catch until the first
    // chat-driven proposal. Fail loud at test time instead.
    let prompt = super::proposer::build_system_prompt(&ConnectionsSnapshot::empty(), 1, None);
    assert!(
        prompt.contains(super::proposer::WORKFLOW_BUILDER_PROMPT_SIGNATURE),
        "bundled workflow_builder.md missing canonical signature line — \
         is `include_str!` resolving to the right file?"
    );
}

#[test]
fn bundled_workflow_builder_prompt_matches_design_time_artifact() {
    // Dual-write expectation: the production prompt at
    // src/openhuman/agent/prompts/workflow_builder.md must stay
    // byte-identical to the design-time artifact at
    // Automations/Artifacts/prompts/workflow_builder.md. A drift
    // here means a future edit hit one path but not both.
    let artifact = include_str!("../../../Automations/Artifacts/prompts/workflow_builder.md");
    let bundled = include_str!("../agent/prompts/workflow_builder.md");
    assert_eq!(
        artifact, bundled,
        "design-time artifact and bundled prompt have drifted; dual-write expected"
    );
}

#[test]
fn bundled_workflow_builder_prompt_is_non_trivial_size() {
    // Catches the "we shipped an empty file" failure mode the
    // signature check doesn't cover (e.g. someone landed a one-line
    // placeholder with the signature inside it).
    let bundled = include_str!("../agent/prompts/workflow_builder.md");
    assert!(
        bundled.len() > 4_000,
        "bundled workflow_builder.md is suspiciously small ({} bytes) — \
         expected the full Phase 1 artifact (~270 lines)",
        bundled.len()
    );
}
