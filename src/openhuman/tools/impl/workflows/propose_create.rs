//! `workflow_propose_create` — propose-only "build me a workflow that …" (F-12).
//!
//! Calls [`proposer::draft_with_retries`] against the F-11 retry
//! loop and returns the resulting [`WorkflowProposal`] as JSON.
//! The proposal is preview-only — F-14's
//! `<WorkflowProposalPreview>` renders it; the user's [Save] or
//! [Save & Enable] click is what triggers `workflows_create` (the
//! single mutation boundary per ADR-012).
//!
//! ## Drafter wiring
//!
//! Built-in `AgentDrafter` is the F-11 placeholder per the F-15
//! swap point. The tool surfaces the underlying [`DraftFailure`]
//! as a structured `{ "error", "kind_label", ... }` JSON payload so
//! the chat agent can render a graceful failure instead of crashing
//! the turn.

use crate::openhuman::config::Config;
use crate::openhuman::connections::aggregator;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::workflows::health::ConnectionsSnapshot;
use crate::openhuman::workflows::proposer::{self, AgentDrafter, Drafter, DEFAULT_MAX_ATTEMPTS};
use crate::openhuman::workflows::types::DraftFailure;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// Phase the propose tool drafts against. Hard-coded to 1 in Phase
/// 1; F-15's surface (`about_app::current_phase()`) will replace
/// this constant so a Phase 2 upgrade is a one-line swap.
const CURRENT_PHASE: u32 = 1;

pub struct WorkflowProposeCreateTool {
    config: Arc<Config>,
    drafter: Arc<dyn Drafter>,
}

impl WorkflowProposeCreateTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config: config.clone(),
            drafter: Arc::new(AgentDrafter::new(config)),
        }
    }

    #[cfg(test)]
    pub fn with_drafter(config: Arc<Config>, drafter: Arc<dyn Drafter>) -> Self {
        Self { config, drafter }
    }
}

#[async_trait]
impl Tool for WorkflowProposeCreateTool {
    fn name(&self) -> &str {
        super::TOOL_WORKFLOW_PROPOSE_CREATE
    }

    fn description(&self) -> &str {
        "PREVIEW-ONLY: take a natural-language description and return a \
         `WorkflowProposal` payload. The user clicks [Save] on the preview \
         card to commit via `workflows_create`. This tool does NOT mutate. \
         On a drafting failure (LLM error, validation failed after 3 \
         retries), returns a structured `{ error, kind_label }` payload."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "User-authored 'build me a workflow that …' sentence."
                }
            },
            "required": ["description"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    fn supports_markdown(&self) -> bool {
        // Markdown-capable so the agent harness picks up the
        // `markdown_formatted` field carrying the
        // `<workflow-preview>` tag the chat-runtime extension
        // (`AgentMessageBubble`) parses + dispatches to
        // `<WorkflowProposalPreview>`.
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let description = args
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("missing required field `description`"))?;
        if description.trim().is_empty() {
            return Ok(ToolResult::success(serde_json::to_string(&json!({
                "error": "empty_description",
            }))?));
        }
        tracing::info!(
            target: "workflows-agent",
            "[workflows-agent] tool=workflow_propose_create desc_len={}",
            description.len()
        );

        let snapshot = match aggregator::list_all(&self.config).await {
            Ok(views) => ConnectionsSnapshot::new(views),
            Err(err) => {
                tracing::warn!(
                    target: "workflows-agent",
                    "[workflows-agent] aggregator failed during propose_create: {err:#}; falling back to empty snapshot"
                );
                ConnectionsSnapshot::empty()
            }
        };

        match proposer::draft_with_retries(
            self.drafter.as_ref(),
            &description,
            &snapshot,
            CURRENT_PHASE,
            DEFAULT_MAX_ATTEMPTS,
        )
        .await
        {
            Ok(proposal) => {
                let payload = json!({ "proposal": proposal });
                let json_str = serde_json::to_string(&proposal)?;
                // Wrap the payload in the `<workflow-preview>` tag the
                // chat-runtime extension (AgentMessageBubble) parses
                // out and dispatches to `<WorkflowProposalPreview>`.
                // Single-quoted attribute so the JSON's double quotes
                // nest cleanly. The instruction at the top tells the
                // LLM to echo this verbatim into its user-facing
                // response so the preview actually renders.
                let markdown = format!(
                    "I drafted this workflow. To show the preview card to the user, \
                     include this tag verbatim in your response:\n\n\
                     <workflow-preview kind=\"proposal\" data='{json_str}'></workflow-preview>"
                );
                Ok(ToolResult::success_with_markdown(payload, markdown))
            }
            Err(DraftFailure::ValidationFailedAfterRetries {
                attempts,
                last_error,
            }) => {
                let payload = json!({
                    "error": "validation_failed_after_retries",
                    "attempts": attempts,
                    "kind_label": last_error.kind_label(),
                    "last_error": last_error,
                });
                Ok(ToolResult::success(serde_json::to_string(&payload)?))
            }
            Err(DraftFailure::RunFailure { reason }) => {
                let payload = json!({
                    "error": "drafting_failed",
                    "kind_label": "run_failure",
                    "reason": reason,
                });
                Ok(ToolResult::success(serde_json::to_string(&payload)?))
            }
        }
    }
}
