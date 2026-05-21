//! Drafting sub-agent + bounded retry loop (ADR-015, FR-1.13).
//!
//! [`draft_with_retries`] is the engine of the chat-driven hero flow:
//! it takes a natural-language description, runs a drafting
//! sub-agent against the [`workflow_builder.md`] system prompt with
//! the live [`ConnectionsSnapshot`] inlined, validates the emitted
//! [`WorkflowProposal`], and on validation failure feeds the
//! structured error back into the next attempt's prompt. Retry
//! budget is 3 (FR-1.13.4) → 90 s worst case (NFR-2.1.4).
//!
//! ## Split with the validator
//!
//! - [`validator::validate`] is **pure** (deterministic, sub-50 ms,
//!   no I/O). It owns every "is this proposal safe to persist?"
//!   check.
//! - This module is **stateful + I/O-bearing** (the LLM call). It
//!   orchestrates retries + composes the system prompt.
//!
//! The split keeps the drafting loop trivially testable via a
//! [`MockDrafter`] without ever needing an LLM endpoint, and lets
//! F-12's `workflow_propose_*` tools share the validator with the
//! UI-side `workflows_create` path.
//!
//! ## Phase 1 invocation placeholder
//!
//! Same constraint F-8 documents: invoking the agent from a
//! non-Turn context (a propose-tool call inside a chat turn is fine,
//! but the F-11 standalone path used by tests / future RPCs is not)
//! requires the `Agent::from_config(...).run_single(prompt)`
//! invocation that F-15's hero E2E will land. Until then,
//! [`AgentDrafter`] is a clearly-labelled placeholder that
//! intentionally returns a `RunFailure` — F-12 + F-14 ship the rest
//! of the wiring against this surface, and F-15 swaps the body
//! without changing the [`Drafter`] trait.
//!
//! [`workflow_builder.md`]: ../../agent/prompts/workflow_builder.md

use crate::openhuman::workflows::health::ConnectionsSnapshot;
use crate::openhuman::workflows::types::{DraftFailure, ProposalValidationError, WorkflowProposal};
use crate::openhuman::workflows::validator;
use async_trait::async_trait;

/// Bundled drafting prompt. F-13 owns the file content + the Tauri
/// resource bundling that ships it alongside the binary; F-11 lands
/// the placeholder so this `include_str!` resolves at build time.
const WORKFLOW_BUILDER_PROMPT: &str = include_str!("../agent/prompts/workflow_builder.md");

/// Default retry budget per ADR-015 / FR-1.13.4.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;

/// Default iteration cap for the drafting sub-agent (FR-1.13.2).
///
/// Distinct from `agent_prompt`'s cap (`12` — set on per-node
/// `AgentPromptConfig`): the drafter only needs to look up
/// connections, optionally inspect existing workflows, and emit one
/// `emit_proposal` call, so the budget is tighter.
pub const DEFAULT_ITERATION_CAP: u32 = 6;

/// Drafting sub-agent's tool allowlist (ADR-016). Stable shape the
/// F-11 tests assert verbatim; the names match the registered tools
/// from F-10 (`workflow_list`) and the always-on Phase 0 tool
/// (`list_connections`). `emit_proposal` is synthetic — the
/// concrete [`Drafter`] impl intercepts it instead of dispatching
/// through `tools::registry`.
pub const DRAFTING_TOOL_ALLOWLIST: &[&str] =
    &["list_connections", "workflow_list", "emit_proposal"];

/// Trait the retry loop calls on every attempt. Production wires
/// [`AgentDrafter`] (which delegates to the agent harness); tests
/// wire `MockDrafter` to script the response sequence
/// deterministically.
///
/// The error surface is [`RunFailure`] — used by the retry loop to
/// distinguish a transient sub-agent failure (no `emit_proposal`,
/// LLM provider error, timeout) from a semantic validation failure.
#[async_trait]
pub trait Drafter: Send + Sync {
    /// Run the drafting sub-agent once. Returns the proposal the
    /// agent emitted via the synthetic `emit_proposal` tool — the
    /// caller validates it before deciding whether to retry.
    ///
    /// `system_prompt` is the full composed prompt
    /// ([`build_system_prompt`] output, including any
    /// previous-attempt error block).
    /// `description` is the user's original "build me a workflow
    /// that …" sentence — passed verbatim, not summarised.
    async fn draft(
        &self,
        system_prompt: &str,
        description: &str,
    ) -> Result<WorkflowProposal, RunFailure>;
}

/// Failure mode the drafting sub-agent surface itself can hit
/// (orthogonal to [`ProposalValidationError`]).
#[derive(Debug, Clone)]
pub struct RunFailure {
    pub reason: String,
}

impl RunFailure {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for RunFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "drafting sub-agent failed: {}", self.reason)
    }
}

impl std::error::Error for RunFailure {}

/// Production-side [`Drafter`]. F-15's hero E2E swaps the body for
/// `Agent::from_config(...).run_single(prompt)` per the dependency
/// survey in the F-8 DEVLOG. Until then this returns a labelled
/// failure so any callers exercising the live path get a stable
/// error code instead of silently looping.
pub struct AgentDrafter;

impl AgentDrafter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AgentDrafter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Drafter for AgentDrafter {
    async fn draft(
        &self,
        _system_prompt: &str,
        _description: &str,
    ) -> Result<WorkflowProposal, RunFailure> {
        // F-15 SWAP POINT — replace this body with the agent
        // invocation. Signature is locked.
        Err(RunFailure::new(
            "AgentDrafter is the F-11 placeholder; live agent invocation lands at F-15.",
        ))
    }
}

/// Run the drafting sub-agent against `description` and the live
/// connections snapshot, retrying on validation failure up to
/// `max_attempts` times.
///
/// On the first `Ok(())` validator return, this returns the
/// proposal. On validator failure, the structured
/// [`ProposalValidationError`] is appended to the next attempt's
/// system prompt via [`build_system_prompt`] so the LLM can fix
/// the typo / dropped field / etc.
///
/// Returns:
///   - `Ok(WorkflowProposal)` — accepted by the validator at
///     attempt `< max_attempts`.
///   - `Err(DraftFailure::ValidationFailedAfterRetries { attempts,
///     last_error })` — every attempt failed validation.
///   - `Err(DraftFailure::RunFailure { reason })` — the sub-agent
///     itself failed (LLM error, no `emit_proposal`, timeout).
///     Distinct from a validation failure so callers can branch
///     on transient vs. semantic errors.
///
/// Pass [`DEFAULT_MAX_ATTEMPTS`] for production calls; tests pin a
/// smaller / larger value to exercise the loop without slowing the
/// suite.
pub async fn draft_with_retries<D: Drafter + ?Sized>(
    drafter: &D,
    description: &str,
    snapshot: &ConnectionsSnapshot,
    phase: u32,
    max_attempts: u32,
) -> Result<WorkflowProposal, DraftFailure> {
    if max_attempts == 0 {
        return Err(DraftFailure::RunFailure {
            reason: "max_attempts must be > 0".into(),
        });
    }
    let preview: String = description.chars().take(80).collect();
    let mut last_error: Option<ProposalValidationError> = None;
    for attempt in 0..max_attempts {
        tracing::info!(
            target: "workflows-proposer",
            "[workflows-proposer] attempt {n}/{max_attempts} description=\"{preview}\"",
            n = attempt + 1,
        );
        let system_prompt = build_system_prompt(snapshot, phase, last_error.as_ref());
        let proposal = match drafter.draft(&system_prompt, description).await {
            Ok(p) => p,
            Err(err) => {
                return Err(DraftFailure::RunFailure { reason: err.reason });
            }
        };
        match validator::validate(&proposal, snapshot, phase) {
            Ok(()) => {
                tracing::info!(
                    target: "workflows-proposer",
                    "[workflows-proposer] attempt {n} accepted",
                    n = attempt + 1
                );
                return Ok(proposal);
            }
            Err(err) => {
                tracing::warn!(
                    target: "workflows-validator",
                    "[workflows-validator] attempt {n} failed kind={kind}",
                    n = attempt + 1,
                    kind = err.kind_label()
                );
                last_error = Some(err);
            }
        }
    }
    Err(DraftFailure::ValidationFailedAfterRetries {
        attempts: max_attempts,
        last_error: last_error
            .expect("loop ran ≥ once and at least one validator error must have been recorded"),
    })
}

/// Compose the drafting sub-agent's system prompt:
///
/// 1. The base [`workflow_builder.md`] content (F-13 owns).
/// 2. A live "Your connections" summary derived from `snapshot`.
/// 3. A "Phase N constraints" block tightening the model's output
///    surface to the kinds [`allowed_node_kinds`] permits.
/// 4. (Optional) A "PREVIOUS ATTEMPT FAILED" block carrying the
///    last [`ProposalValidationError`] verbatim so the next
///    attempt can correct typos / dropped fields without guessing.
///
/// Pure function — no I/O — so tests can assert the composed
/// string content directly.
///
/// [`allowed_node_kinds`]: validator::allowed_node_kinds
/// [`workflow_builder.md`]: ../../agent/prompts/workflow_builder.md
pub fn build_system_prompt(
    snapshot: &ConnectionsSnapshot,
    phase: u32,
    last_error: Option<&ProposalValidationError>,
) -> String {
    let mut prompt = WORKFLOW_BUILDER_PROMPT.to_string();
    prompt.push_str("\n\n## Your connections\n\n");
    prompt.push_str(&summarize_connections(snapshot));
    prompt.push_str("\n\n## Phase ");
    prompt.push_str(&phase.to_string());
    prompt.push_str(" constraints\n\n");
    prompt.push_str(&phase_constraints_block(phase));
    if let Some(err) = last_error {
        prompt.push_str("\n\n## PREVIOUS ATTEMPT FAILED\n\n");
        prompt.push_str(&format_validation_error(err));
    }
    prompt
}

/// One-line-per-mechanism human-readable connection summary. Used
/// inside the drafting prompt; the agent uses `list_connections`
/// for live fallback (ADR-009 hybrid discovery).
fn summarize_connections(snapshot: &ConnectionsSnapshot) -> String {
    if snapshot.is_empty() {
        return "_You have no connections yet. Suggest connecting one before proposing a workflow that requires it._".into();
    }
    use crate::openhuman::connections::types::ConnectionRef;
    let mut composio: Vec<&str> = Vec::new();
    let mut channel: Vec<&str> = Vec::new();
    let mut webview: Vec<&str> = Vec::new();
    let mut builtin: Vec<&str> = Vec::new();
    let mut mcp: Vec<&str> = Vec::new();
    let mut http: Vec<&str> = Vec::new();
    for v in snapshot.views() {
        match &v.r#ref {
            ConnectionRef::Composio { toolkit_id, .. } => composio.push(toolkit_id),
            ConnectionRef::Channel { provider, .. } => channel.push(provider),
            ConnectionRef::Webview { provider, .. } => webview.push(provider),
            ConnectionRef::Builtin { integration } => builtin.push(integration),
            ConnectionRef::Mcp { server_id, .. } => mcp.push(server_id),
            ConnectionRef::GenericHttp { connection_id } => http.push(connection_id),
        }
    }
    let mut out = String::new();
    push_group(&mut out, "Composio", &composio);
    push_group(&mut out, "Channels", &channel);
    push_group(&mut out, "Webview accounts", &webview);
    push_group(&mut out, "Built-in", &builtin);
    push_group(&mut out, "MCP servers", &mcp);
    push_group(&mut out, "Generic HTTP", &http);
    out
}

fn push_group(out: &mut String, label: &str, items: &[&str]) {
    if items.is_empty() {
        return;
    }
    out.push_str("- **");
    out.push_str(label);
    out.push_str("**: ");
    out.push_str(&items.join(", "));
    out.push('\n');
}

fn phase_constraints_block(phase: u32) -> String {
    let kinds = validator::allowed_node_kinds(phase);
    let kind_list: Vec<String> = kinds.iter().map(|k| format!("{k:?}")).collect();
    format!(
        "- Allowed node kinds: {}\n- Allowed triggers: Cron, Manual\n- on_error policy: Halt (Phase 1)\n- timeout_secs clamp: [1, 3600]",
        kind_list.join(", ")
    )
}

/// Render a [`ProposalValidationError`] as a structured block the
/// drafting agent can read and respond to in its retry attempt.
/// Stays deliberately terse — no proposal content here (NFR-2.4.4).
fn format_validation_error(err: &ProposalValidationError) -> String {
    use ProposalValidationError as E;
    match err {
        E::JsonParse { reason } => format!("- error_kind: json_parse\n- reason: {reason}"),
        E::MissingRequiredField { field } => {
            format!("- error_kind: missing_required_field\n- field: {field}")
        }
        E::UnsupportedNodeKind { node_kind, phase } => format!(
            "- error_kind: unsupported_node_kind\n- node_kind: {node_kind:?}\n- phase: {phase}"
        ),
        E::InvalidCron { expr, parse_error } => {
            format!("- error_kind: invalid_cron\n- expr: {expr}\n- parse_error: {parse_error}")
        }
        E::EdgeIntegrity { from, to, reason } => {
            format!("- error_kind: edge_integrity\n- from: {from}\n- to: {to}\n- reason: {reason}")
        }
        E::UnknownConnection { r#ref, candidates } => {
            let names: Vec<String> = candidates.iter().map(|c| format!("{c:?}")).collect();
            format!(
                "- error_kind: unknown_connection\n- ref: {ref_:?}\n- candidates: [{cands}]",
                ref_ = r#ref,
                cands = names.join(", "),
            )
        }
    }
}
