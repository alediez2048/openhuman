//! F3-3 — `browser_observe` tool.
//!
//! "What's on the page right now?" — calls
//! [`crate::openhuman::browser_agent::perceive::snapshot`] on the
//! session resolved from the [`SessionRegistry`], renders the result
//! at the requested detail tier, returns the rendered text as the
//! markdown payload + a JSON sidecar with the structured snapshot.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::openhuman::browser_agent::perceive::{snapshot, DetailTier, SnapshotOptions};
use crate::openhuman::browser_agent::registry::SessionRegistry;
use crate::openhuman::tools::traits::{
    PermissionLevel, Tool, ToolCallOptions, ToolCategory, ToolResult,
};

pub struct BrowserObserveTool;

impl BrowserObserveTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BrowserObserveTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BrowserObserveTool {
    fn name(&self) -> &str {
        super::constants::TOOL_BROWSER_OBSERVE
    }

    fn description(&self) -> &str {
        "Observe the current browser page. Returns the page URL, title, and a list of \
         actionable elements (`[N] role \"label\"`). Address elements by `[N]` in \
         subsequent `browser_act` / `browser_extract` calls. Set `detail` to \
         `compact` (~500 tokens, top 30 elements only), `standard` (default, every \
         element + short text excerpt), or `verbose` (every element + full attribute \
         dump + larger text excerpt). Use `verbose` only when you need to disambiguate \
         between similar elements."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "user_id": {
                    "type": "string",
                    "description": "Workflow run's user id (injected by the executor)."
                },
                "run_id": {
                    "type": "string",
                    "description": "Workflow run id (injected by the executor)."
                },
                "detail": {
                    "type": "string",
                    "enum": ["compact", "standard", "verbose"],
                    "default": "standard"
                }
            },
            "required": ["user_id", "run_id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolResult> {
        self.execute_with_options(args, ToolCallOptions::default())
            .await
    }

    async fn execute_with_options(
        &self,
        args: Value,
        _options: ToolCallOptions,
    ) -> Result<ToolResult> {
        let Some(user_id) = args.get("user_id").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("browser_observe: `user_id` required"));
        };
        let Some(run_id) = args.get("run_id").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("browser_observe: `run_id` required"));
        };
        let tier = parse_tier(args.get("detail"));

        let Some(session) =
            SessionRegistry::instance().get(&user_id.to_string(), &run_id.to_string())
        else {
            return Ok(ToolResult::error(
                "browser_observe: no active session for this run. \
                 The BrowserAction node (F3-4) opens one on dispatch — \
                 calling observe before the node ran is the usual cause.",
            ));
        };

        // F3-6 chunk 3: wall-clock cost cap. Checked BEFORE any
        // LLM-fanout work so a tripped cap doesn't pay for one more
        // snapshot before halting.
        if let Some(short_circuit) = check_wall_clock_cap(user_id, run_id, "browser_observe", &args)
        {
            return Ok(short_circuit);
        }

        let snap = match snapshot(&session, SnapshotOptions::default()).await {
            Ok(s) => s,
            Err(e) => {
                // F3-6 chunk 2: best-effort audit even on the error
                // path so the run-detail UI surfaces failed observe
                // attempts.
                emit_audit(
                    user_id,
                    run_id,
                    "browser_observe",
                    &args,
                    &format!("error: {e}"),
                );
                return Ok(ToolResult::error(format!("browser_observe: {e}")));
            }
        };
        let rendered = crate::openhuman::browser_agent::perceive::to_llm_text(&snap, tier);
        let payload = json!({
            "url": snap.url,
            "title": snap.title,
            "element_count": snap.elements.len(),
            "token_estimate": snap.snapshot_token_estimate,
        });
        emit_audit(
            user_id,
            run_id,
            "browser_observe",
            &args,
            &format!(
                "observed {} elements at {} ({})",
                snap.elements.len(),
                snap.url,
                format!("{tier:?}").to_lowercase()
            ),
        );
        Ok(ToolResult::success_with_markdown(payload, rendered))
    }
}

/// F3-6 chunk 3: shared wall-clock cap check. Tools call this at
/// the top of `execute` BEFORE doing any LLM-fanout work (DOM
/// snapshot, CDP click, regex parse). When the cap is exceeded the
/// tool returns a structured `cost_cap_exceeded` ToolResult and the
/// agent halts cleanly. Audit-logged with `[wall_clock_exceeded]`
/// prefix so post-mortem inspection shows the trip point.
///
/// Returns `Some(error_result)` to short-circuit, `None` to continue.
pub(super) fn check_wall_clock_cap(
    user_id: &str,
    run_id: &str,
    tool_name: &str,
    args: &Value,
) -> Option<ToolResult> {
    let meta = SessionRegistry::instance().get_meta(&user_id.to_string(), &run_id.to_string());
    let cap = meta.wall_clock_cap?;
    if !cap.is_exceeded() {
        return None;
    }
    let elapsed = cap.started_at.elapsed().as_secs();
    let body = json!({
        "status": "cost_cap_exceeded",
        "which": "wall_clock",
        "elapsed_secs": elapsed,
        "max_secs": cap.max_secs,
    });
    let markdown = format!(
        "[COST CAP] wall_clock exceeded: {elapsed}s elapsed vs {}s allowed; stopping.",
        cap.max_secs
    );
    emit_audit(
        user_id,
        run_id,
        tool_name,
        args,
        &format!("[wall_clock_exceeded] {elapsed}s/{}s", cap.max_secs),
    );
    Some(ToolResult::success_with_markdown(body, markdown))
}

/// F3-6 chunk 2: shared audit-log writer. Reads `workspace_dir` from
/// the per-run `RunMeta`. No-op when meta has no workspace (tests
/// without the executor having installed one, or runs where the audit
/// is intentionally disabled). Best-effort — failures are logged +
/// swallowed inside `write_entry_at`.
pub(super) fn emit_audit(
    user_id: &str,
    run_id: &str,
    tool_name: &str,
    args: &Value,
    result_summary: &str,
) {
    let meta = SessionRegistry::instance().get_meta(&user_id.to_string(), &run_id.to_string());
    let Some(workspace) = meta.workspace_dir else {
        return;
    };
    let args_json = serde_json::to_string(args).unwrap_or_else(|_| "{}".into());
    // F3-6 chunk 4a: sweep sensitive fields out of args before
    // persistence. Each redacted field counts toward
    // `redacted_fields_count` on the row so the run-detail UI can
    // surface that redaction ran.
    let (redacted_args, redacted_count) =
        crate::openhuman::browser_agent::safety::redact_args_str(&args_json);
    let mut entry = crate::openhuman::browser_agent::safety::AuditLogEntry::new(
        run_id,
        tool_name,
        redacted_args,
        result_summary,
    );
    entry.redacted_fields_count = redacted_count;
    let _ = crate::openhuman::browser_agent::safety::audit_log::write_entry_at(&workspace, entry);
}

fn parse_tier(v: Option<&Value>) -> DetailTier {
    match v.and_then(|x| x.as_str()).unwrap_or("standard") {
        "compact" => DetailTier::Compact,
        "verbose" => DetailTier::Verbose,
        _ => DetailTier::Standard,
    }
}
