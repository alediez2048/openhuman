//! `workflow_propose_update` — propose-only "edit this workflow"
//! preview (F-12).
//!
//! Fetches the current [`Workflow`], calls
//! [`proposer::draft_with_retries_for_update`] (which inlines the
//! current shape into the system prompt), then assembles a
//! [`WorkflowEditProposal { current, proposed, diff_summary,
//! rationale }`] payload. The diff is computed server-side via
//! [`workflows::diff::workflow_diff`] so the UI doesn't reinvent it.
//!
//! Does NOT mutate (ADR-012). The user's [Save changes] click on
//! the preview component is what calls `workflows_update`.

use crate::openhuman::config::Config;
use crate::openhuman::connections::aggregator;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::workflows::diff;
use crate::openhuman::workflows::health::ConnectionsSnapshot;
use crate::openhuman::workflows::ops;
use crate::openhuman::workflows::proposer::{
    self, AgentUpdateDrafter, UpdateDrafter, DEFAULT_MAX_ATTEMPTS,
};
use crate::openhuman::workflows::types::{DraftFailure, WorkflowEditProposal};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

const CURRENT_PHASE: u32 = 1;

pub struct WorkflowProposeUpdateTool {
    config: Arc<Config>,
    drafter: Arc<dyn UpdateDrafter>,
}

impl WorkflowProposeUpdateTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config: config.clone(),
            drafter: Arc::new(AgentUpdateDrafter::new(config)),
        }
    }

    #[cfg(test)]
    pub fn with_drafter(config: Arc<Config>, drafter: Arc<dyn UpdateDrafter>) -> Self {
        Self { config, drafter }
    }
}

#[async_trait]
impl Tool for WorkflowProposeUpdateTool {
    fn name(&self) -> &str {
        super::TOOL_WORKFLOW_PROPOSE_UPDATE
    }

    fn description(&self) -> &str {
        "PREVIEW-ONLY: take a workflow id + edit instructions and \
         return a `WorkflowEditProposal { current, proposed, diff_summary, \
         rationale }` payload. The UI renders the diff; the user's \
         [Save changes] click commits via `workflows_update`. This tool \
         does NOT mutate."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": { "type": "string", "description": "Workflow id to edit." },
                "instructions": {
                    "type": "string",
                    "description": "Natural-language edit ('change the schedule to 9am')."
                }
            },
            "required": ["workflow_id", "instructions"],
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
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let workflow_id = args
            .get("workflow_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("missing required field `workflow_id`"))?;
        let instructions = args
            .get("instructions")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("missing required field `instructions`"))?;
        tracing::info!(
            target: "workflows-agent",
            "[workflows-agent] tool=workflow_propose_update wf={workflow_id} instr_len={}",
            instructions.len()
        );

        let current = match ops::get(&self.config, workflow_id.clone())
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .value
        {
            Some(w) => w,
            None => {
                return Ok(ToolResult::success(serde_json::to_string(&json!({
                    "error": "not_found",
                    "workflow_id": workflow_id,
                }))?))
            }
        };

        let snapshot = match aggregator::list_all(&self.config).await {
            Ok(views) => ConnectionsSnapshot::new(views),
            Err(err) => {
                tracing::warn!(
                    target: "workflows-agent",
                    "[workflows-agent] aggregator failed during propose_update: {err:#}; falling back to empty snapshot"
                );
                ConnectionsSnapshot::empty()
            }
        };

        match proposer::draft_with_retries_for_update(
            self.drafter.as_ref(),
            &instructions,
            &current,
            &snapshot,
            CURRENT_PHASE,
            DEFAULT_MAX_ATTEMPTS,
        )
        .await
        {
            Ok(proposed) => {
                let diff_summary = diff::workflow_diff(&current, &proposed);
                let rationale = if diff_summary.is_empty() {
                    vec!["No changes to apply.".into()]
                } else {
                    vec![]
                };
                let preview = WorkflowEditProposal {
                    workflow_id: current.id.clone(),
                    current,
                    proposed,
                    diff_summary,
                    rationale,
                };
                let json_str = serde_json::to_string(&preview)?;
                let data_b64 =
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, json_str.as_bytes());
                let preview_tag = format!(
                    "<workflow-preview kind=\"edit\" data=\"{data_b64}\"></workflow-preview>"
                );
                let payload = json!({
                    "status": "draft_ready",
                    "render_instructions": "Include the `preview_tag` value verbatim in your user-facing reply. Do not call workflow_propose_update again — the user clicks Save on the rendered diff card to commit.",
                    "preview_tag": preview_tag,
                    "edit_proposal": preview,
                });
                let markdown = format!(
                    "Draft ready. Include this tag verbatim in your reply, then stop — do NOT call this tool again.\n\n{preview_tag}"
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
