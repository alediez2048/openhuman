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
//! ## LLM invocation — direct provider, not the agent harness
//!
//! Both [`AgentDrafter::draft`] and [`AgentUpdateDrafter::draft_update`]
//! issue a one-shot `Provider::chat_with_system` call against the
//! `"agentic"` workload provider — bypassing the agent harness
//! entirely. Earlier iterations routed through
//! `Agent::from_config(config).run_single(...)`, but that defaults
//! to the **orchestrator** agent whose identity ("you never write
//! code, delegate only when needed") conflicts with the drafter's
//! contract of emitting a fenced ```json``` proposal block. Live
//! testing showed the orchestrator returning natural-language
//! refusals / delegation attempts instead of the required JSON,
//! exhausting `draft_with_retries`' budget on every chat-driven
//! create attempt.
//!
//! The drafter is a one-shot LLM call with a precise system prompt
//! ([`build_system_prompt`] composes
//! `workflow_builder.md` + live connections snapshot + any prior-attempt
//! validation error + the OUTPUT FORMAT instruction) and a tightly-
//! structured JSON output contract — no tools, no iteration, no
//! agent identity. `chat_with_system` is the right primitive.
//! [`parse_proposal_from_response`] / [`extract_fenced_json`] pull
//! the JSON out of the response and validate it.
//!
//! [`workflow_builder.md`]: ../../agent/prompts/workflow_builder.md

use crate::openhuman::workflows::health::ConnectionsSnapshot;
use crate::openhuman::workflows::types::{
    DraftFailure, ProposalValidationError, Workflow, WorkflowProposal,
};
use crate::openhuman::workflows::validator;
use async_trait::async_trait;

/// Bundled drafting prompt.
///
/// Compile-time `include_str!` embeds the canonical content from
/// `src/openhuman/agent/prompts/workflow_builder.md` into the
/// binary's text segment so the proposer has zero runtime
/// dependency on the filesystem. The same directory is ALSO bundled
/// as a Tauri resource via `app/src-tauri/tauri.conf.json`'s
/// `resources` glob (`../../src/openhuman/agent/prompts`), which
/// ships the file inside `<App>.app/Contents/Resources/...` for
/// discoverability by QA + future dev-tools.
///
/// Design-time source of truth lives at
/// `Automations/Artifacts/prompts/workflow_builder.md`; F-13
/// promoted the artifact to this production path byte-for-byte.
/// Future edits dual-write both files until a follow-up
/// symlink ticket lands.
const WORKFLOW_BUILDER_PROMPT: &str = include_str!("../agent/prompts/workflow_builder.md");

/// Stable substring drawn from the bundled prompt's opening
/// paragraph (`Automations/Artifacts/prompts/workflow_builder.md`
/// line 13). The F-13 smoke test asserts this is present so a
/// build that accidentally picked up an empty / wrong-path file
/// fails fast at test time rather than at the first chat-driven
/// proposal.
#[doc(hidden)]
pub const WORKFLOW_BUILDER_PROMPT_SIGNATURE: &str =
    "drafting sub-agent** for OpenHuman's Workflows feature";

/// Fire a single info-level log line the first time the proposer
/// composes a system prompt. Subsequent calls are silent — the
/// log answers "is the bundled prompt loaded and at what size?"
/// once at startup-equivalent, then gets out of the way.
fn log_prompt_load_once() {
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    tracing::info!(
        target: "workflows-proposer",
        "[workflows-proposer] loaded workflow_builder.md ({n} chars, signature_present={present})",
        n = WORKFLOW_BUILDER_PROMPT.len(),
        present = WORKFLOW_BUILDER_PROMPT.contains(WORKFLOW_BUILDER_PROMPT_SIGNATURE)
    );
}

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

/// Production-side [`Drafter`]. Builds an [`Agent`] from the project
/// config and calls `run_single(system_prompt + description)` — the
/// same pattern `cron::scheduler::handle_scheduled_job` uses. Parses
/// the agent's text response for a fenced ```json``` block carrying
/// a [`WorkflowProposal`].
///
/// The prompt's "Output format" instruction (appended by
/// [`build_system_prompt`]) tells the LLM to emit:
///
/// ````
/// ```json
/// { ...WorkflowProposal... }
/// ```
/// ````
///
/// — and nothing else. Multiple/missing blocks surface as
/// [`RunFailure`]; the retry loop in [`draft_with_retries`] handles
/// that as a transient error (vs. a validator-level
/// [`ProposalValidationError`]).
pub struct AgentDrafter {
    config: std::sync::Arc<crate::openhuman::config::Config>,
}

impl AgentDrafter {
    pub fn new(config: std::sync::Arc<crate::openhuman::config::Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Drafter for AgentDrafter {
    async fn draft(
        &self,
        system_prompt: &str,
        description: &str,
    ) -> Result<WorkflowProposal, RunFailure> {
        let response =
            run_agent_for_text(&self.config, system_prompt, description, "proposal").await?;
        parse_proposal_from_response(&response)
    }
}

/// Shared LLM invocation used by both [`AgentDrafter`] and
/// [`AgentUpdateDrafter`].
///
/// **Bypasses the agent harness entirely.** Earlier iterations went
/// through `Agent::from_config(config).run_single(...)`, but that
/// defaults to the **orchestrator** agent definition — whose system
/// prompt declares "you never write code / never directly modify
/// files / delegate only when needed". The composed drafter prompt
/// (workflow_builder.md + user description) conflicts with that
/// identity, and live testing showed the model returning natural-
/// language refusals / delegation rather than the required fenced
/// `json` proposal block, causing `draft_with_retries` to exhaust
/// its budget every time.
///
/// The drafter is fundamentally a one-shot LLM call with a precise
/// system prompt and a tightly-structured JSON output contract — no
/// tools, no iteration, no agent identity. The right primitive is
/// [`Provider::chat_with_system`] directly. We route through the
/// `"agentic"` workload factory so the user's configured drafting
/// model (typically the same one the orchestrator uses for chat
/// reasoning) is picked up automatically.
async fn run_agent_for_text(
    config: &crate::openhuman::config::Config,
    system_prompt: &str,
    description: &str,
    kind: &str,
) -> Result<String, RunFailure> {
    let (provider, model) =
        crate::openhuman::inference::provider::create_chat_provider("agentic", config)
            .map_err(|e| RunFailure::new(format!("create_chat_provider(agentic) failed: {e:#}")))?;
    tracing::info!(
        target: "workflows-proposer",
        kind = %kind,
        model = %model,
        system_prompt_chars = system_prompt.len(),
        description_chars = description.len(),
        "[workflows-proposer] dispatching one-shot drafter LLM call"
    );
    let response = provider
        .chat_with_system(Some(system_prompt), description, &model, 0.2)
        .await
        .map_err(|e| RunFailure::new(format!("provider.chat_with_system failed: {e:#}")))?;
    // Surface the raw LLM response so when parsing fails we can SEE
    // what came back instead of guessing. First 800 chars are usually
    // enough to spot "refusal text", "prose instead of JSON", "missing
    // closing fence", etc. — and 800 chars keep the log line readable.
    let preview: String = response.chars().take(800).collect();
    tracing::info!(
        target: "workflows-proposer",
        kind = %kind,
        model = %model,
        response_chars = response.len(),
        has_json_fence = response.contains("```json") || response.contains("```\n{"),
        response_preview = %preview,
        "[workflows-proposer] drafter LLM response received"
    );
    Ok(response)
}

/// Extract a [`WorkflowProposal`] from the LLM's text response.
/// Looks for a fenced ```json ...``` block (the standard agent
/// output format per the appended "Output format" instruction).
/// Falls back to parsing the whole response as raw JSON when no
/// fence is found — the LLM occasionally returns the JSON inline.
fn parse_proposal_from_response(text: &str) -> Result<WorkflowProposal, RunFailure> {
    let fenced = extract_fenced_json(text);
    let body = fenced.unwrap_or_else(|| text.trim());
    serde_json::from_str::<WorkflowProposal>(body).map_err(|err| {
        let body_preview: String = body.chars().take(400).collect();
        tracing::warn!(
            target: "workflows-proposer",
            err = %err,
            had_fence = fenced.is_some(),
            body_chars = body.len(),
            body_preview = %body_preview,
            "[workflows-proposer] parse_proposal_from_response: serde_json::from_str FAILED"
        );
        RunFailure::new(format!(
            "proposal payload not parseable as WorkflowProposal: {err}"
        ))
    })
}

/// Pull the body of the first ```json (or bare ```) fenced block
/// out of `text`. Returns `None` when no fence is present so the
/// caller can fall back to raw-JSON parsing.
fn extract_fenced_json(text: &str) -> Option<&str> {
    let start = text.find("```json").or_else(|| text.find("```"))?;
    // Skip past the opening fence + optional `json` language hint +
    // the newline that follows.
    let after_open = &text[start..];
    let body_start = after_open.find('\n')? + start + 1;
    let end_rel = text[body_start..].find("```")?;
    Some(text[body_start..body_start + end_rel].trim())
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
                tracing::warn!(
                    target: "workflows-proposer",
                    attempt = attempt + 1,
                    reason = %err.reason,
                    "[workflows-proposer] drafter.draft FAILED — aborting retry loop with RunFailure"
                );
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
    log_prompt_load_once();
    let mut prompt = WORKFLOW_BUILDER_PROMPT.to_string();
    prompt.push_str("\n\n## Your connections\n\n");
    prompt.push_str(&summarize_connections(snapshot));
    prompt.push_str("\n\n## Phase ");
    prompt.push_str(&phase.to_string());
    prompt.push_str(" constraints\n\n");
    prompt.push_str(&phase_constraints_block(phase));
    prompt.push_str(OUTPUT_FORMAT_INSTRUCTION);
    if let Some(err) = last_error {
        prompt.push_str("\n\n## PREVIOUS ATTEMPT FAILED\n\n");
        prompt.push_str(&format_validation_error(err));
    }
    prompt
}

/// Critical-override section appended to the system prompt for both
/// the create + update drafters. The locked
/// `Automations/Artifacts/prompts/workflow_builder.md` artifact
/// instructs the LLM to "emit via `emit_proposal`", but that
/// synthetic tool isn't registered in the agent harness — the F-11
/// integration plan called for it but landing the registration is
/// bigger than the F-15 scope. Instead we ask the LLM to emit the
/// proposal as a fenced ```json``` code block; the drafter's
/// [`parse_proposal_from_response`] / `extract_fenced_json` pulls
/// the body out and validates it.
///
/// The instruction is appended LAST (after the connections summary
/// + phase constraints + previous-attempt block) so it's the most
/// recent context the LLM sees — that's the highest-recency win
/// against the artifact's earlier `emit_proposal` references.
const OUTPUT_FORMAT_INSTRUCTION: &str = "\n\n## Output format (CRITICAL OVERRIDE)\n\n\
The `emit_proposal` tool is NOT available in this session. Override the artifact's \
`emit_proposal` instructions with the following rule:\n\n\
- Your ENTIRE response must be a single fenced JSON code block containing the \
  `WorkflowProposal` payload. No prose before or after the block.\n\
- The fence opens with three backticks then `json`, and closes with three backticks.\n\
- Fields required: `name`, `description`, `trigger`, `nodes`, `edges`, `settings`, \
  `required_connections`, `rationale`, `confidence`.\n\
- For update flows the same rule applies but the payload is a full `Workflow` shape \
  (with `id`, `schema_version`, `enabled`, `origin`, `health`, `created_at`, \
  `updated_at` preserved from the current workflow).\n\
- Stay strictly under ~2 KiB of JSON. Don't summarise the prompt; emit the workflow.\n";

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

// ── Update sibling (F-12) ──────────────────────────────────────────────

/// Sibling of [`Drafter`] for edit-style proposals. The signature
/// returns a full [`Workflow`] (the proposed shape) rather than a
/// [`WorkflowProposal`]: edits land against an existing row's id,
/// so the diff payload is `(current, proposed)` rather than the
/// freshly-drafted shape.
#[async_trait]
pub trait UpdateDrafter: Send + Sync {
    /// Run the update-drafting sub-agent once with the current
    /// workflow JSON inlined into `system_prompt` and the user's
    /// edit instructions passed as `instructions`. Returns the
    /// agent's proposed [`Workflow`] shape (validation happens
    /// outside).
    async fn draft_update(
        &self,
        system_prompt: &str,
        instructions: &str,
        current: &Workflow,
    ) -> Result<Workflow, RunFailure>;
}

/// Production-side [`UpdateDrafter`]. Same swap pattern as
/// [`AgentDrafter`]: calls `Agent::from_config(config).run_single(...)`
/// with the update-flavoured system prompt (current workflow JSON
/// inlined) + the user's edit instructions; parses a fenced ```json
/// block out of the response as the proposed [`Workflow`] shape.
pub struct AgentUpdateDrafter {
    config: std::sync::Arc<crate::openhuman::config::Config>,
}

impl AgentUpdateDrafter {
    pub fn new(config: std::sync::Arc<crate::openhuman::config::Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl UpdateDrafter for AgentUpdateDrafter {
    async fn draft_update(
        &self,
        system_prompt: &str,
        instructions: &str,
        _current: &Workflow,
    ) -> Result<Workflow, RunFailure> {
        let response =
            run_agent_for_text(&self.config, system_prompt, instructions, "edit").await?;
        let body = extract_fenced_json(&response).unwrap_or(response.trim());
        serde_json::from_str::<Workflow>(body).map_err(|err| {
            RunFailure::new(format!("edit payload not parseable as Workflow: {err}"))
        })
    }
}

/// Sibling of [`draft_with_retries`] for edit-style proposals.
/// Inlines the current workflow JSON into the system prompt
/// (via [`build_update_system_prompt`]) and walks the same
/// validate-or-retry loop as the create path.
///
/// On success returns the proposed [`Workflow`] (caller assembles
/// the `WorkflowEditProposal` by diffing against `current`).
pub async fn draft_with_retries_for_update<D: UpdateDrafter + ?Sized>(
    drafter: &D,
    instructions: &str,
    current: &Workflow,
    snapshot: &ConnectionsSnapshot,
    phase: u32,
    max_attempts: u32,
) -> Result<Workflow, DraftFailure> {
    if max_attempts == 0 {
        return Err(DraftFailure::RunFailure {
            reason: "max_attempts must be > 0".into(),
        });
    }
    let preview: String = instructions.chars().take(80).collect();
    let mut last_error: Option<ProposalValidationError> = None;
    for attempt in 0..max_attempts {
        tracing::info!(
            target: "workflows-proposer",
            "[workflows-proposer] update attempt {n}/{max_attempts} wf={} instructions=\"{preview}\"",
            current.id,
            n = attempt + 1,
        );
        let system_prompt =
            build_update_system_prompt(snapshot, phase, current, last_error.as_ref());
        let proposed = match drafter
            .draft_update(&system_prompt, instructions, current)
            .await
        {
            Ok(w) => w,
            Err(err) => {
                return Err(DraftFailure::RunFailure { reason: err.reason });
            }
        };
        // Reuse the same validator: it walks the same checks
        // against the proposed shape's nodes / edges / triggers.
        // The validator API takes a `WorkflowProposal`; project the
        // proposed `Workflow` into that shape for validation only.
        let projected = WorkflowProposal {
            name: proposed.name.clone(),
            description: proposed.description.clone().unwrap_or_default(),
            trigger: proposed.trigger.clone(),
            nodes: proposed.nodes.clone(),
            edges: proposed.edges.clone(),
            settings: proposed.settings.clone(),
            required_connections: collect_required_connections(&proposed),
            rationale: vec![],
            confidence: crate::openhuman::workflows::types::Confidence::Medium,
        };
        match validator::validate(&projected, snapshot, phase) {
            Ok(()) => {
                tracing::info!(
                    target: "workflows-proposer",
                    "[workflows-proposer] update attempt {n} accepted",
                    n = attempt + 1
                );
                return Ok(proposed);
            }
            Err(err) => {
                tracing::warn!(
                    target: "workflows-validator",
                    "[workflows-validator] update attempt {n} failed kind={kind}",
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

/// Compose the update-drafting system prompt: same base as
/// [`build_system_prompt`] plus a "Current workflow" block
/// carrying the existing row's JSON so the model can edit
/// surgically rather than re-draft from scratch.
pub fn build_update_system_prompt(
    snapshot: &ConnectionsSnapshot,
    phase: u32,
    current: &Workflow,
    last_error: Option<&ProposalValidationError>,
) -> String {
    let mut prompt = build_system_prompt(snapshot, phase, last_error);
    prompt.push_str("\n\n## Current workflow\n\n");
    prompt.push_str("```json\n");
    match serde_json::to_string_pretty(current) {
        Ok(s) => prompt.push_str(&s),
        Err(err) => {
            // Should never fail — the workflow round-trips through
            // SQLite as JSON. Log and fall back to a stable
            // placeholder so the prompt stays valid.
            tracing::warn!(
                target: "workflows-proposer",
                "[workflows-proposer] failed to serialise current workflow for update prompt: {err:#}"
            );
            prompt.push_str("{ /* serialise error */ }");
        }
    }
    prompt.push_str("\n```\n");
    prompt.push_str("\nApply the user's instructions to this workflow. Return the FULL proposed workflow shape (same fields), not a diff.\n");
    prompt
}

/// Union of every `ConnectionRef` referenced anywhere in the
/// proposed workflow's `agent_prompt.allowed_connections`. Used to
/// populate `WorkflowProposal::required_connections` when projecting
/// a `Workflow` through the create-shape validator.
fn collect_required_connections(
    wf: &Workflow,
) -> Vec<crate::openhuman::connections::types::ConnectionRef> {
    let mut out: Vec<crate::openhuman::connections::types::ConnectionRef> = Vec::new();
    for node in &wf.nodes {
        let crate::openhuman::workflows::types::NodeConfig::AgentPrompt(cfg) = &node.config;
        for r in &cfg.allowed_connections {
            if !out.contains(r) {
                out.push(r.clone());
            }
        }
    }
    out
}
