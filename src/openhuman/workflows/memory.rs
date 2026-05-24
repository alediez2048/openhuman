//! Workflow-run memory schema (F-17): the ground-truth-first chunk that
//! the executor stores in the Memory Tree after every run, and that
//! pre-run recall reads back into the next agent invocation.
//!
//! ## Why this lives here
//!
//! Phase 1 shipped the **doer** half of OpenHuman's "memory + doer"
//! thesis; F-17 closes the loop. The executor (`executor.rs`) calls into
//! this module twice per run:
//!
//!   1. **Pre-run** — `Memory::recall("workflow:{id}", limit=3)` returns
//!      `Vec<MemoryEntry>`; each entry's `content` is parsed back into a
//!      [`WorkflowRunMemory`] via [`WorkflowRunMemory::parse_storage_markdown`]
//!      and rendered as a single bullet via
//!      [`WorkflowRunMemory::to_recall_line`] (the user-visible "Prior
//!      runs of this workflow" block prepended to the next agent prompt).
//!   2. **Post-run** — after `agent.run_single` returns, the executor
//!      builds a [`WorkflowRunMemory`] from:
//!      - the F-16 event-bus tap (extended in deliverable C to capture
//!        per-call detail, not just a failure count),
//!      - the agent's final response (the `narrative`),
//!      - the workflow's `allowed_connections` (auto entity tags),
//!      - the agent's optional `## Entities touched` section (agent
//!        entity tags).
//!      Serialises via [`WorkflowRunMemory::to_storage_markdown`] and
//!      writes via `Memory::store`.
//!
//! ## Ground-truth-first
//!
//! [`WorkflowRunMemory::actual`] is built from the tool-call trace; the
//! agent's text is `narrative`. When they disagree
//! ([`compute_drift`] returns drift entries) the recall rendering
//! surfaces the drift explicitly so the next-run agent sees the prior
//! lie rather than inheriting it.
//!
//! ## Entity tags (forward-compat Phase 5 hook)
//!
//! Tags are free-form `entity:<kind>:<id>` strings. Phase 5 will
//! enforce a vocabulary; F-17 ships the convention without enforcement
//! so the schemas can emerge from observed data. The runtime
//! auto-tags every workflow with the toolkit/provider it touches; the
//! agent can opt in to richer domain tags via the
//! `## Entities touched` section in its final response.
//!
//! See `Automations/Tickets/phase-1-foundation/F-17.md` for the full
//! specification including the rationale for ground-truth-first + the
//! drift detector design.

use crate::openhuman::connections::types::ConnectionRef;
use crate::openhuman::memory::traits::MemoryCategory;
use crate::openhuman::workflows::types::{RunId, RunStatus, TriggerSource, WorkflowId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Memory namespace prefix every workflow run is stored under. Per F-17,
/// each workflow id gets its own namespace: `workflow/{workflow_id}`.
/// Stored chunks share that namespace; the per-run key is
/// `run:{run_id}` so a query-by-namespace returns one entry per run.
///
/// We use `/` rather than the spec's `:` separator because
/// `UnifiedMemory::sanitize_namespace` strips characters outside
/// `[A-Za-z0-9_/-]`; a colon would silently become an underscore at
/// write time and break read-back through `Memory::get` (which does
/// not re-sanitize). Per-run KEYS retain `:` — they aren't subject
/// to the same sanitiser.
pub const WORKFLOW_MEMORY_NAMESPACE_PREFIX: &str = "workflow/";

/// Recall token budget for the "## Prior runs of this workflow" block
/// prepended to the agent's user prompt. ~1200 tokens at the rough 4
/// chars/token ratio. Tunable via [`render_recall_block`].
pub const RECALL_BLOCK_MAX_CHARS: usize = 4800;

/// Memory category used for every workflow-run chunk. `Daily` matches
/// the existing per-day temporal organisation of the Memory Tree —
/// per-run records are operational logs, not foundational facts.
pub const WORKFLOW_MEMORY_CATEGORY: MemoryCategory = MemoryCategory::Daily;

/// Build the namespace string for a workflow id.
pub fn namespace_for(workflow_id: &str) -> String {
    format!("{WORKFLOW_MEMORY_NAMESPACE_PREFIX}{workflow_id}")
}

/// Build the per-run key inside the workflow namespace.
pub fn key_for_run(run_id: &str) -> String {
    format!("run:{run_id}")
}

/// One workflow-run summary stored as a Memory Tree chunk under
/// `workflow:{workflow_id}`. Per-run; the recall surface keeps the
/// last 3 by recency.
///
/// **Wire shape:** the structure serialises to JSON for the trailing
/// fenced block of the storage markdown (see
/// [`Self::to_storage_markdown`]). The markdown above the fence is a
/// human-readable mirror; the JSON below is the parseable source of
/// truth that recall reads back via
/// [`Self::parse_storage_markdown`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRunMemory {
    pub workflow_id: WorkflowId,
    pub run_id: RunId,
    pub triggered_at: DateTime<Utc>,
    pub trigger_source: TriggerSource,
    /// Honest terminal status per F-16's gate. A run where the agent
    /// claimed "Done!" but `tool_failure_count > 0` is recorded as
    /// `Failed` here; never inferred from `narrative`.
    pub status: RunStatus,

    /// Ground truth — built from the F-16 event-bus tap, not the
    /// agent's text. Source of truth for cross-checks + next-run
    /// recall.
    pub actual: ActualOutcome,

    /// The agent's final text response. One-shot summary; truncated to
    /// 600 chars to keep recall blocks bounded.
    pub narrative: String,

    /// `false` iff the heuristic in [`compute_drift`] detected a
    /// concrete mismatch between `narrative` and `actual`.
    pub narrative_matches_actual: bool,

    /// When `narrative_matches_actual = false`, one bullet per detected
    /// mismatch ("Narrative claims action 'sent' but
    /// composio_execute(SLACK_SEND_MESSAGE) failed with
    /// rate_limit_exceeded"). Empty otherwise.
    #[serde(default)]
    pub narrative_drift: Vec<String>,

    /// Entity tags this run touched. Format: `entity:<kind>:<id>`. The
    /// kind vocabulary is intentionally not enforced — Phase 5 will
    /// canonicalise based on what actually emerges in production. Auto
    /// tags from connections (`entity:source:slack`) merge with
    /// agent-authored tags from the `## Entities touched` section.
    #[serde(default)]
    pub entity_tags: Vec<String>,
}

/// Trace-derived "what actually happened" half of [`WorkflowRunMemory`].
/// Built by the executor's event-bus subscriber, never by the agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ActualOutcome {
    /// Every `ToolExecutionCompleted` event observed during the run,
    /// scoped to the run's session id. Ordered chronologically.
    #[serde(default)]
    pub tool_calls: Vec<ToolCallTrace>,
    /// Outcomes the trace can PROVE happened (a successful tool call
    /// matched to a verb claim in the narrative + cross-checked).
    /// Populated by the post-run builder, not deserialised from the
    /// agent's text.
    #[serde(default)]
    pub side_effects_confirmed: Vec<String>,
    /// Outcomes the agent claimed but the trace can't confirm. Kept
    /// here for the cross-check audit trail — NOT promoted to
    /// "happened".
    #[serde(default)]
    pub side_effects_claimed_unverified: Vec<String>,
    /// Failures, denials, cost-cap trips, dry-runs. Source-of-truth
    /// for the recall block's ⚠ annotation.
    #[serde(default)]
    pub anomalies: Vec<String>,
}

/// One tool-call entry inside [`ActualOutcome::tool_calls`]. Built from
/// the F-16 event-bus tap (extended in deliverable C to carry
/// `redacted_args` and `inner_status` alongside the existing
/// `tool_name` / `success` / `elapsed_ms`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallTrace {
    pub tool_name: String,
    /// Arguments after a simple regex redaction pass (see
    /// [`redact_args`]). Phase 1.5 redaction is intentionally
    /// minimal; F4-6 will replace this with the structured policy.
    #[serde(default)]
    pub redacted_args: serde_json::Value,
    pub success: bool,
    pub elapsed_ms: u64,
    /// Optional inner-status payload (e.g. `"rate_limit_exceeded"`,
    /// `"cost_cap_exceeded"`). Surfaced by the harness when the tool
    /// returned a structured error; `None` for plain success or for
    /// failures without a status string.
    #[serde(default)]
    pub inner_status: Option<String>,
}

impl WorkflowRunMemory {
    /// Serialise this memory chunk as canonical Markdown. The format
    /// is dual-purpose: humans reading the Obsidian mirror see the
    /// structured top half; recall reads the trailing JSON fence to
    /// reconstruct the full struct without losing fidelity.
    ///
    /// Stability: the order of sections is fixed; tests assert verbatim
    /// substrings to keep the contract explicit. Adding a new section
    /// requires updating
    /// [`Self::parse_storage_markdown`] in lock-step.
    pub fn to_storage_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "# Workflow run: {} / {}\n",
            self.workflow_id, self.run_id
        ));
        out.push_str(&format!(
            "**Status:** {:?} | **Triggered:** {} | **Source:** {}\n\n",
            self.status,
            self.triggered_at.format("%Y-%m-%d %H:%M UTC"),
            trigger_source_label(&self.trigger_source)
        ));

        out.push_str("## Narrative\n");
        if self.narrative.is_empty() {
            out.push_str("_(empty)_\n\n");
        } else {
            out.push_str(&self.narrative);
            out.push_str("\n\n");
        }

        out.push_str("## Actual\n");
        if self.actual.tool_calls.is_empty() {
            out.push_str("_(no tool calls observed)_\n");
        } else {
            for call in &self.actual.tool_calls {
                let marker = if call.success { "✓" } else { "✗" };
                let status_suffix = call
                    .inner_status
                    .as_deref()
                    .map(|s| format!(": {s}"))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "- {} {} ({}ms){}\n",
                    marker, call.tool_name, call.elapsed_ms, status_suffix
                ));
            }
        }
        if !self.actual.anomalies.is_empty() {
            out.push_str("\n### Anomalies\n");
            for a in &self.actual.anomalies {
                out.push_str(&format!("- {a}\n"));
            }
        }
        out.push('\n');

        out.push_str("## Entities\n");
        if self.entity_tags.is_empty() {
            out.push_str("_(none)_\n");
        } else {
            for tag in &self.entity_tags {
                out.push_str(&format!("- {tag}\n"));
            }
        }
        out.push('\n');

        if !self.narrative_matches_actual {
            out.push_str("## Drift\n");
            for d in &self.narrative_drift {
                out.push_str(&format!("- {d}\n"));
            }
            out.push('\n');
        }

        // Trailing JSON fence — the parseable source of truth.
        out.push_str("```json\n");
        match serde_json::to_string_pretty(self) {
            Ok(j) => out.push_str(&j),
            Err(_) => out.push_str("{}"),
        }
        out.push_str("\n```\n");
        out
    }

    /// Re-parse a chunk previously written by
    /// [`Self::to_storage_markdown`]. Reads the trailing JSON fence;
    /// the human-readable markdown above is ignored.
    pub fn parse_storage_markdown(content: &str) -> Option<Self> {
        let fence_open = content.rfind("```json")?;
        let after_open = &content[fence_open + "```json".len()..];
        let fence_close = after_open.find("```")?;
        let json_block = after_open[..fence_close].trim();
        serde_json::from_str(json_block).ok()
    }

    /// Render this memory as ONE Markdown bullet for the pre-run recall
    /// block (deliverable B). Format mirrors the spec's example:
    /// timestamp, status, ground-truth `Actual` line, optional ⚠
    /// drift annotation when the narrative was untrustworthy.
    pub fn to_recall_line(&self) -> String {
        let ts = self.triggered_at.format("%Y-%m-%d %H:%M");
        let status = format!("{:?}", self.status);
        let actual = recall_actual_phrase(&self.actual);
        let mut line = format!("- **{ts}** ({status}) — Actual: {actual}.");
        if self.narrative_matches_actual && !self.narrative.is_empty() {
            // Trim to keep the bullet bounded; recall caps the whole
            // block at ~1200 tokens (deliverable B).
            let trimmed = truncate_for_recall(&self.narrative, 240);
            line.push_str(&format!(" Narrative: {trimmed}"));
        } else if !self.narrative_drift.is_empty() {
            let drift_join = self.narrative_drift.join("; ");
            line.push_str(&format!(
                " ⚠ Narrative drift: {drift_join} — DO NOT assume the narrative is true."
            ));
        }
        line
    }
}

/// Auto-tag a workflow's `allowed_connections` into
/// `entity:source:<toolkit>` strings. Always-included; no opt-out.
/// Order is preserved; duplicates within the input collapse on the
/// caller side via [`merge_entity_tags`].
pub fn auto_entity_tags(connections: &[ConnectionRef]) -> Vec<String> {
    let mut tags = Vec::new();
    for r in connections {
        match r {
            ConnectionRef::Composio { toolkit_id, .. } => {
                tags.push(format!("entity:source:{}", toolkit_id.to_lowercase()));
            }
            ConnectionRef::Channel { provider, .. } => {
                tags.push(format!("entity:source:{}", provider.to_lowercase()));
            }
            ConnectionRef::Webview { provider, .. } => {
                tags.push(format!("entity:source:{}", provider.to_lowercase()));
            }
            ConnectionRef::Builtin { integration } => {
                tags.push(format!("entity:source:{}", integration.to_lowercase()));
            }
            ConnectionRef::Mcp { server_id, .. } => {
                tags.push(format!("entity:source:mcp:{}", server_id.to_lowercase()));
            }
            ConnectionRef::GenericHttp { connection_id } => {
                tags.push(format!("entity:source:http:{}", connection_id.to_lowercase()));
            }
        }
    }
    tags
}

/// Parse the optional `## Entities touched` section from an agent's
/// final response. Each bullet should be one `entity:<kind>:<id>`
/// string; the parser is tolerant — a malformed or missing section
/// returns an empty vec rather than erroring.
///
/// Recognised forms:
///   - `## Entities touched` (canonical)
///   - `## Entities`  (shorthand the agent sometimes uses)
///
/// Inside the section, leading `-` / `*` / `+` bullets are stripped.
/// Lines that don't start with `entity:` after trimming are skipped.
pub fn parse_agent_entity_tags(text: &str) -> Vec<String> {
    const HEADERS: &[&str] = &["## Entities touched", "## Entities"];
    let mut start: Option<usize> = None;
    for header in HEADERS {
        if let Some(pos) = text.find(header) {
            // Move past the header line.
            let after = pos + header.len();
            let rest_start = text[after..]
                .find('\n')
                .map(|nl| after + nl + 1)
                .unwrap_or(text.len());
            start = Some(rest_start);
            break;
        }
    }
    let Some(section_start) = start else {
        return Vec::new();
    };

    let section = &text[section_start..];
    // Stop at the next markdown header (line starting with `## `) so we
    // don't accidentally swallow content from a following section.
    let section_end = section
        .lines()
        .scan(0usize, |off, line| {
            let line_start = *off;
            *off += line.len() + 1;
            Some((line_start, line))
        })
        .find(|(_, line)| line.trim_start().starts_with("## "))
        .map(|(off, _)| off);
    let body = match section_end {
        Some(end) => &section[..end],
        None => section,
    };

    let mut tags = Vec::new();
    for raw_line in body.lines() {
        let line = raw_line
            .trim()
            .trim_start_matches(|c: char| c == '-' || c == '*' || c == '+')
            .trim();
        if line.starts_with("entity:") && line.len() > "entity:".len() {
            tags.push(line.to_string());
        }
    }
    tags
}

/// Merge auto-tags with agent-authored tags. Order: auto first (stable
/// for tests), then agent additions in the order they appeared. Dups
/// collapse to the first occurrence.
pub fn merge_entity_tags(auto: Vec<String>, agent: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(auto.len() + agent.len());
    for t in auto.into_iter().chain(agent.into_iter()) {
        if seen.insert(t.clone()) {
            out.push(t);
        }
    }
    out
}

/// Drift heuristic: does `narrative` claim something `actual` can't
/// support? Returns `(matches, drift_entries)`.
///
/// Phase 1.5 implementation is intentionally simple — regex/substring
/// only. The high-value case is "agent claims success-verbs while
/// every tool call failed". A semantic-similarity version is a future
/// ticket per F-17's architectural decisions.
///
/// Returns:
///   - `(true, [])` — no drift detected.
///   - `(false, ["Narrative claims action '...' but ..."])` — one or
///     more concrete mismatches.
pub fn compute_drift(narrative: &str, actual: &ActualOutcome) -> (bool, Vec<String>) {
    const SUCCESS_VERBS: &[&str] = &[
        "sent",
        "created",
        "scheduled",
        "delivered",
        "posted",
        "shared",
        "updated",
        "deleted",
        "added",
    ];
    if narrative.is_empty() {
        return (true, Vec::new());
    }
    let lower = narrative.to_lowercase();

    let any_success = actual.tool_calls.iter().any(|c| c.success);
    let failures: Vec<&ToolCallTrace> = actual.tool_calls.iter().filter(|c| !c.success).collect();

    // Only a confabulation hazard if there were failed tool calls and
    // NO successful calls. (If some calls succeeded, give the agent
    // the benefit of the doubt — a smarter classifier would diff
    // per-action.)
    if any_success || failures.is_empty() {
        return (true, Vec::new());
    }

    let mut drift = Vec::new();
    for verb in SUCCESS_VERBS {
        if !lower.contains(verb) {
            continue;
        }
        // Skip explicit negations so "couldn't send" / "did not send"
        // don't trigger drift.
        let negations = [
            format!("not {verb}"),
            format!("didn't {verb}"),
            format!("did not {verb}"),
            format!("couldn't {verb}"),
            format!("could not {verb}"),
            format!("failed to {verb}"),
        ];
        if negations.iter().any(|n| lower.contains(n.as_str())) {
            continue;
        }
        for call in &failures {
            let status_suffix = call
                .inner_status
                .as_deref()
                .map(|s| format!(" with {s}"))
                .unwrap_or_default();
            drift.push(format!(
                "Narrative claims action '{verb}' but {} failed{}",
                call.tool_name, status_suffix
            ));
        }
        // One verb's worth of drift is enough to flip the flag — keep
        // the list short. Stop after the first verb-with-failure.
        break;
    }

    if drift.is_empty() {
        (true, Vec::new())
    } else {
        (false, drift)
    }
}

/// Phase 1.5 redaction policy for `ToolCallTrace::redacted_args`:
/// a simple recursive walk that masks fields whose JSON-key name
/// matches a small allow-list of "obvious" secret-bearing names. F4-6
/// will replace this with the structured policy + password-field
/// detection.
///
/// Masked keys (case-insensitive):
///   - `password`, `secret`, `token`, `api_key`, `apikey`, `bearer`,
///     `authorization`, `auth`, `private_key`, `client_secret`
///
/// String values matching obvious secret-shaped patterns (JWTs, long
/// base64 blobs) are NOT detected here — that's F4-6's job.
pub fn redact_args(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    const MASKED_KEYS: &[&str] = &[
        "password",
        "secret",
        "token",
        "api_key",
        "apikey",
        "bearer",
        "authorization",
        "auth",
        "private_key",
        "client_secret",
    ];
    fn matches_masked(key: &str) -> bool {
        let lower = key.to_lowercase();
        MASKED_KEYS.iter().any(|m| lower.contains(m))
    }
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if matches_masked(k) {
                    out.insert(k.clone(), Value::String("[redacted]".into()));
                } else {
                    out.insert(k.clone(), redact_args(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_args).collect()),
        other => other.clone(),
    }
}

// ── Internal helpers ────────────────────────────────────────────────────

fn trigger_source_label(source: &TriggerSource) -> String {
    match source {
        TriggerSource::Cron => "Cron".into(),
        TriggerSource::Manual { initiator } => format!("Manual ({initiator})"),
        TriggerSource::Webhook => "Webhook".into(),
        TriggerSource::ComposioEvent => "ComposioEvent".into(),
        TriggerSource::ChannelMessage => "ChannelMessage".into(),
    }
}

/// Build a compact "Actual" phrase for [`WorkflowRunMemory::to_recall_line`].
/// Lists successful tool calls inline with their names; failures get
/// a `FAILED` marker so the next-run agent reads them as anomalies.
fn recall_actual_phrase(actual: &ActualOutcome) -> String {
    if actual.tool_calls.is_empty() {
        return "no tool calls observed".into();
    }
    let parts: Vec<String> = actual
        .tool_calls
        .iter()
        .map(|c| {
            if c.success {
                format!("{} success", c.tool_name)
            } else {
                let inner = c
                    .inner_status
                    .as_deref()
                    .map(|s| format!(" ({s})"))
                    .unwrap_or_default();
                format!("{} FAILED{}", c.tool_name, inner)
            }
        })
        .collect();
    parts.join("; ")
}

fn truncate_for_recall(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…")
}

// ── Pre-run recall (deliverable B) ──────────────────────────────────────

/// Fetch the N most-recent prior-run summaries for the given workflow id
/// from the process-global memory client. Returned newest-first.
///
/// Reads directly from `memory_docs` via the `Memory::sqlite_conn()`
/// escape hatch — `Memory::list` is the obvious choice but its
/// `UnifiedMemory` impl maps `title → content` (a documented quirk),
/// so it would surface a title, not the canonical markdown chunk we
/// wrote in [`persist_run_memory`]. Going straight to SQL keeps the
/// recall path honest while still letting other backends (memory
/// mocks in tests, future remote backends) implement `sqlite_conn()`
/// if they want to participate. Backends without a sqlite connection
/// recall as empty — same best-effort posture as the rest of F-17.
///
/// **Best-effort:** when the global memory client isn't initialised
/// (some unit-test workspaces skip it), this returns an empty vec
/// without erroring. A failed query logs a warn and returns empty.
/// The executor proceeds with no recall context rather than failing
/// the workflow run.
pub async fn recall_prior_runs(workflow_id: &str, limit: usize) -> Vec<WorkflowRunMemory> {
    let Some(client) = crate::openhuman::memory::global::client_if_ready() else {
        tracing::debug!(
            target: "workflows-memory",
            "[workflows-memory] no global memory client; treating as no prior runs"
        );
        return Vec::new();
    };
    let Some(conn) = client.memory_handle().sqlite_conn() else {
        tracing::debug!(
            target: "workflows-memory",
            "[workflows-memory] memory backend has no sqlite connection; recall returns empty"
        );
        return Vec::new();
    };
    let namespace = namespace_for(workflow_id);
    let rows: Vec<String> = {
        let conn = conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT content FROM memory_docs
             WHERE namespace = ?1
             ORDER BY updated_at DESC
             LIMIT ?2",
        ) {
            Ok(stmt) => stmt,
            Err(err) => {
                tracing::warn!(
                    target: "workflows-memory",
                    "[workflows-memory] prepare recall query failed: {err:#}"
                );
                return Vec::new();
            }
        };
        let mapped = stmt.query_map(rusqlite::params![namespace, limit as i64], |row| {
            row.get::<_, String>(0)
        });
        match mapped {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(err) => {
                tracing::warn!(
                    target: "workflows-memory",
                    "[workflows-memory] recall query failed for ns={namespace}: {err:#}"
                );
                return Vec::new();
            }
        }
    };

    rows.into_iter()
        .filter_map(|content| WorkflowRunMemory::parse_storage_markdown(&content))
        .collect()
}

/// Render the pre-run recall block prepended to the agent's user prompt.
/// Newest first. When `prior_runs` is empty, returns the explicit
/// "first execution" fallback line so the LLM understands it's not
/// missing context.
///
/// Token budget: caps total chars at `max_chars`; if appending the next
/// line would exceed the cap, stops there (the oldest bullets drop).
/// `max_chars` defaults to [`RECALL_BLOCK_MAX_CHARS`] in production
/// callers; tests override.
pub fn render_recall_block(prior_runs: &[WorkflowRunMemory], max_chars: usize) -> String {
    if prior_runs.is_empty() {
        return "## No prior runs — this is the first execution.\n".to_string();
    }
    let header = "## Prior runs of this workflow\n\n";
    let mut block = String::from(header);
    let mut emitted = 0usize;
    for run in prior_runs {
        let line = run.to_recall_line();
        // +1 for the trailing newline we'll append.
        if block.len() + line.len() + 1 > max_chars && emitted > 0 {
            tracing::debug!(
                target: "workflows-memory",
                "[workflows-memory] recall block hit char cap ({max_chars}); \
                 dropped {} oldest entries",
                prior_runs.len() - emitted
            );
            break;
        }
        block.push_str(&line);
        block.push('\n');
        emitted += 1;
    }
    block
}

/// Compose the agent's user-message prompt by prepending the recall
/// block to the workflow's authored prompt. A blank line separates the
/// two so the LLM reads the recall as preamble rather than running it
/// into the instructions.
pub fn compose_prompt_with_recall(recall_block: &str, authored_prompt: &str) -> String {
    let trimmed_block = recall_block.trim_end();
    if trimmed_block.is_empty() {
        return authored_prompt.to_string();
    }
    format!("{trimmed_block}\n\n{authored_prompt}")
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn sample_memory() -> WorkflowRunMemory {
        WorkflowRunMemory {
            workflow_id: "wf-1".into(),
            run_id: "run-1".into(),
            triggered_at: Utc.with_ymd_and_hms(2026, 5, 21, 8, 0, 0).unwrap(),
            trigger_source: TriggerSource::Cron,
            status: RunStatus::Succeeded,
            actual: ActualOutcome {
                tool_calls: vec![ToolCallTrace {
                    tool_name: "composio_execute(SLACK_SEND_MESSAGE)".into(),
                    redacted_args: json!({"channel": "C123", "text": "[redacted]"}),
                    success: true,
                    elapsed_ms: 412,
                    inner_status: None,
                }],
                side_effects_confirmed: vec!["Sent Slack DM".into()],
                side_effects_claimed_unverified: vec![],
                anomalies: vec![],
            },
            narrative: "Sent the morning digest to #general.".into(),
            narrative_matches_actual: true,
            narrative_drift: vec![],
            entity_tags: vec!["entity:source:slack".into()],
        }
    }

    #[test]
    fn to_storage_markdown_emits_stable_sections() {
        let mem = sample_memory();
        let md = mem.to_storage_markdown();
        assert!(md.contains("# Workflow run: wf-1 / run-1"));
        assert!(md.contains("**Status:** Succeeded"));
        assert!(md.contains("**Triggered:** 2026-05-21 08:00 UTC"));
        assert!(md.contains("**Source:** Cron"));
        assert!(md.contains("## Narrative\nSent the morning digest to #general."));
        assert!(md.contains(
            "## Actual\n- ✓ composio_execute(SLACK_SEND_MESSAGE) (412ms)"
        ));
        assert!(md.contains("## Entities\n- entity:source:slack"));
        assert!(md.contains("```json\n"));
        // No drift section when narrative_matches_actual = true.
        assert!(!md.contains("## Drift"));
    }

    #[test]
    fn storage_markdown_roundtrips_via_json_fence() {
        let mem = sample_memory();
        let md = mem.to_storage_markdown();
        let parsed = WorkflowRunMemory::parse_storage_markdown(&md)
            .expect("recall must parse what we wrote");
        assert_eq!(parsed, mem);
    }

    #[test]
    fn drift_section_renders_when_narrative_lies() {
        let mut mem = sample_memory();
        mem.narrative_matches_actual = false;
        mem.narrative_drift = vec![
            "Narrative claims action 'sent' but composio_execute(SLACK_SEND_MESSAGE) failed with rate_limit_exceeded".into(),
        ];
        mem.actual.tool_calls[0].success = false;
        mem.actual.tool_calls[0].inner_status = Some("rate_limit_exceeded".into());
        mem.status = RunStatus::Failed;
        let md = mem.to_storage_markdown();
        assert!(md.contains("## Drift\n"));
        assert!(md.contains("rate_limit_exceeded"));
        assert!(md.contains("- ✗ composio_execute(SLACK_SEND_MESSAGE) (412ms): rate_limit_exceeded"));
    }

    #[test]
    fn to_recall_line_happy_path_includes_narrative() {
        let line = sample_memory().to_recall_line();
        assert!(line.starts_with("- **2026-05-21 08:00** (Succeeded)"));
        assert!(line.contains("composio_execute(SLACK_SEND_MESSAGE) success"));
        assert!(line.contains("Narrative: Sent the morning digest"));
        assert!(!line.contains("⚠"));
    }

    #[test]
    fn to_recall_line_drift_path_replaces_narrative_with_warning() {
        let mut mem = sample_memory();
        mem.narrative_matches_actual = false;
        mem.narrative_drift = vec![
            "Narrative claims action 'sent' but composio_execute(SLACK_SEND_MESSAGE) failed with rate_limit_exceeded".into(),
        ];
        mem.actual.tool_calls[0].success = false;
        mem.actual.tool_calls[0].inner_status = Some("rate_limit_exceeded".into());
        mem.status = RunStatus::Failed;
        let line = mem.to_recall_line();
        assert!(line.contains("⚠ Narrative drift:"));
        assert!(line.contains("DO NOT assume the narrative is true"));
        assert!(line.contains("composio_execute(SLACK_SEND_MESSAGE) FAILED (rate_limit_exceeded)"));
        // Narrative text must NOT appear when drift fires — that's the
        // whole point of the cross-check.
        assert!(!line.contains("Sent the morning digest"));
    }

    #[test]
    fn auto_entity_tags_covers_all_six_mechanisms() {
        let refs = vec![
            ConnectionRef::Composio {
                toolkit_id: "Stripe".into(),
                account_id: None,
            },
            ConnectionRef::Channel {
                provider: "slack".into(),
                channel_id: "C1".into(),
            },
            ConnectionRef::Webview {
                provider: "linkedin".into(),
                account_id: "acct".into(),
            },
            ConnectionRef::Builtin {
                integration: "twilio".into(),
            },
            ConnectionRef::Mcp {
                server_id: "srv-1".into(),
                tool_name: None,
            },
            ConnectionRef::GenericHttp {
                connection_id: "conn-1".into(),
            },
        ];
        let tags = auto_entity_tags(&refs);
        assert_eq!(
            tags,
            vec![
                "entity:source:stripe".to_string(),
                "entity:source:slack".to_string(),
                "entity:source:linkedin".to_string(),
                "entity:source:twilio".to_string(),
                "entity:source:mcp:srv-1".to_string(),
                "entity:source:http:conn-1".to_string(),
            ]
        );
    }

    #[test]
    fn parse_agent_entity_tags_extracts_canonical_section() {
        let text = "All done. Here's what I touched:\n\n## Entities touched\n- entity:lead:acme-corp\n- entity:deal:acme-q3-2026\n- entity:meeting:2026-05-28T16:00\n";
        let tags = parse_agent_entity_tags(text);
        assert_eq!(
            tags,
            vec![
                "entity:lead:acme-corp".to_string(),
                "entity:deal:acme-q3-2026".to_string(),
                "entity:meeting:2026-05-28T16:00".to_string(),
            ]
        );
    }

    #[test]
    fn parse_agent_entity_tags_tolerates_shorthand_and_bullet_variants() {
        let text = "Summary.\n\n## Entities\n* entity:lead:foo\n+ entity:deal:bar\n";
        let tags = parse_agent_entity_tags(text);
        assert_eq!(tags, vec!["entity:lead:foo", "entity:deal:bar"]);
    }

    #[test]
    fn parse_agent_entity_tags_returns_empty_for_missing_or_malformed() {
        // No section at all.
        assert!(parse_agent_entity_tags("Just a summary, nothing else.").is_empty());
        // Section exists but body has no `entity:` lines.
        let mistyped = "## Entities touched\n- LEAD: acme-corp\n- random text\n";
        assert!(parse_agent_entity_tags(mistyped).is_empty());
    }

    #[test]
    fn parse_agent_entity_tags_stops_at_next_header() {
        let text = "## Entities touched\n- entity:lead:foo\n\n## Next section\n- entity:should:not:appear\n";
        let tags = parse_agent_entity_tags(text);
        assert_eq!(tags, vec!["entity:lead:foo"]);
    }

    #[test]
    fn merge_entity_tags_dedups_preserving_order() {
        let auto = vec!["entity:source:slack".into(), "entity:source:gmail".into()];
        let agent = vec![
            "entity:source:slack".into(), // dup
            "entity:lead:acme".into(),
            "entity:deal:q3".into(),
        ];
        let merged = merge_entity_tags(auto, agent);
        assert_eq!(
            merged,
            vec![
                "entity:source:slack".to_string(),
                "entity:source:gmail".to_string(),
                "entity:lead:acme".to_string(),
                "entity:deal:q3".to_string(),
            ]
        );
    }

    #[test]
    fn compute_drift_happy_path_returns_no_drift() {
        let narrative = "Sent the morning digest to #general.";
        let actual = ActualOutcome {
            tool_calls: vec![ToolCallTrace {
                tool_name: "composio_execute(SLACK_SEND_MESSAGE)".into(),
                redacted_args: json!({}),
                success: true,
                elapsed_ms: 412,
                inner_status: None,
            }],
            ..Default::default()
        };
        let (matches, drift) = compute_drift(narrative, &actual);
        assert!(matches);
        assert!(drift.is_empty());
    }

    #[test]
    fn compute_drift_catches_lying_about_send() {
        let narrative = "Sent the digest. All good.";
        let actual = ActualOutcome {
            tool_calls: vec![ToolCallTrace {
                tool_name: "composio_execute(SLACK_SEND_MESSAGE)".into(),
                redacted_args: json!({}),
                success: false,
                elapsed_ms: 230,
                inner_status: Some("rate_limit_exceeded".into()),
            }],
            ..Default::default()
        };
        let (matches, drift) = compute_drift(narrative, &actual);
        assert!(!matches);
        assert_eq!(drift.len(), 1);
        assert!(drift[0].contains("sent"));
        assert!(drift[0].contains("composio_execute(SLACK_SEND_MESSAGE)"));
        assert!(drift[0].contains("rate_limit_exceeded"));
    }

    #[test]
    fn compute_drift_skips_explicit_negations() {
        // "couldn't send" should NOT trigger drift — the agent is being
        // honest about the failure.
        let narrative = "I couldn't send the digest because the Slack tool failed.";
        let actual = ActualOutcome {
            tool_calls: vec![ToolCallTrace {
                tool_name: "composio_execute(SLACK_SEND_MESSAGE)".into(),
                redacted_args: json!({}),
                success: false,
                elapsed_ms: 230,
                inner_status: Some("rate_limit_exceeded".into()),
            }],
            ..Default::default()
        };
        let (matches, drift) = compute_drift(narrative, &actual);
        assert!(matches, "negated verb should not register as drift");
        assert!(drift.is_empty());
    }

    #[test]
    fn compute_drift_gives_benefit_of_doubt_on_partial_success() {
        // Mixed success — assume the narrative is talking about the
        // successful call; defer richer matching to a future ticket.
        let narrative = "Sent the digest and updated the spreadsheet.";
        let actual = ActualOutcome {
            tool_calls: vec![
                ToolCallTrace {
                    tool_name: "composio_execute(SLACK_SEND_MESSAGE)".into(),
                    redacted_args: json!({}),
                    success: true,
                    elapsed_ms: 100,
                    inner_status: None,
                },
                ToolCallTrace {
                    tool_name: "composio_execute(GSHEETS_UPDATE)".into(),
                    redacted_args: json!({}),
                    success: false,
                    elapsed_ms: 200,
                    inner_status: Some("permission_denied".into()),
                },
            ],
            ..Default::default()
        };
        let (matches, drift) = compute_drift(narrative, &actual);
        assert!(matches, "phase 1.5 heuristic stays simple");
        assert!(drift.is_empty());
    }

    #[test]
    fn render_recall_block_returns_first_execution_line_when_empty() {
        let block = render_recall_block(&[], RECALL_BLOCK_MAX_CHARS);
        assert_eq!(block, "## No prior runs — this is the first execution.\n");
    }

    #[test]
    fn render_recall_block_emits_header_and_one_bullet_per_run() {
        let mut runs = Vec::new();
        for i in 0..3 {
            let mut m = sample_memory();
            m.run_id = format!("run-{i}");
            m.triggered_at = Utc.with_ymd_and_hms(2026, 5, 19 + i as u32, 8, 0, 0).unwrap();
            runs.push(m);
        }
        let block = render_recall_block(&runs, RECALL_BLOCK_MAX_CHARS);
        assert!(block.starts_with("## Prior runs of this workflow\n\n"));
        // Newest first: each line begins with `- **YYYY-MM-DD HH:MM**`.
        let bullets: Vec<&str> = block.lines().filter(|l| l.starts_with("- **")).collect();
        assert_eq!(bullets.len(), 3);
        assert!(bullets[0].contains("2026-05-19 08:00"));
        assert!(bullets[2].contains("2026-05-21 08:00"));
    }

    #[test]
    fn render_recall_block_drops_oldest_when_over_char_cap() {
        // Build 5 runs each ~ 250 chars; cap at 600 so only 1-2 fit.
        let mut runs = Vec::new();
        for i in 0..5 {
            let mut m = sample_memory();
            m.run_id = format!("run-{i}");
            m.triggered_at = Utc.with_ymd_and_hms(2026, 5, 15 + i as u32, 8, 0, 0).unwrap();
            // Pad narrative so each bullet is well above 50 chars.
            m.narrative =
                "Sent the morning digest, skipped 8 promos, all good.".to_string();
            runs.push(m);
        }
        let block = render_recall_block(&runs, 600);
        let bullets: Vec<&str> = block.lines().filter(|l| l.starts_with("- **")).collect();
        // At least one bullet is always emitted; total cap is respected.
        assert!(!bullets.is_empty());
        assert!(bullets.len() < 5);
        assert!(block.len() <= 600 + 80, "block must respect cap (got {} chars)", block.len());
    }

    #[test]
    fn compose_prompt_with_recall_prefixes_block_above_authored() {
        let block = "## Prior runs of this workflow\n\n- one\n";
        let authored = "Run the digest now.";
        let composed = compose_prompt_with_recall(block, authored);
        assert!(composed.starts_with("## Prior runs of this workflow\n\n- one"));
        assert!(composed.ends_with("Run the digest now."));
        // Blank line between recall and authored prompt.
        assert!(composed.contains("- one\n\nRun the digest now."));
    }

    #[test]
    fn compose_prompt_with_recall_passes_through_when_recall_block_empty() {
        let composed = compose_prompt_with_recall("", "Authored prompt.");
        assert_eq!(composed, "Authored prompt.");
    }

    #[test]
    fn namespace_and_key_helpers_match_address_pattern() {
        // Slash separator (not colon) — UnifiedMemory's
        // sanitize_namespace would strip `:` to `_`, breaking
        // Memory::get round-trips.
        assert_eq!(namespace_for("wf-1"), "workflow/wf-1");
        assert_eq!(key_for_run("r-1"), "run:r-1");
    }

    #[test]
    fn redact_args_masks_sensitive_keys_recursively() {
        let value = json!({
            "channel": "C123",
            "auth": {
                "token": "ghp_secret",
                "user": "alice"
            },
            "items": [
                {"api_key": "shouldhide", "name": "ok"},
                {"name": "still_ok"}
            ],
            "Authorization": "Bearer xxx"
        });
        let redacted = redact_args(&value);
        assert_eq!(redacted["channel"], json!("C123"));
        assert_eq!(redacted["auth"], json!("[redacted]"));
        assert_eq!(redacted["items"][0]["api_key"], json!("[redacted]"));
        assert_eq!(redacted["items"][0]["name"], json!("ok"));
        assert_eq!(redacted["items"][1]["name"], json!("still_ok"));
        assert_eq!(redacted["Authorization"], json!("[redacted]"));
    }
}
