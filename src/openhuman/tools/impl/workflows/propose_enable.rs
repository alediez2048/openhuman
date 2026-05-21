//! `workflow_propose_enable` — propose-only enable preview (F-12).
//!
//! Returns a [`WorkflowStateProposal { action: Enable, rationale }`]
//! the F-14 `<WorkflowProposalPreview>` component renders as a
//! "Enable workflow X?" card. The user's [Apply] click on that card
//! is the ONLY path to mutation — this tool itself does NOT call
//! `workflows_enable` (ADR-012 single-mutation-boundary contract).
//!
//! Skips the LLM: enable / disable rationale is static and
//! deterministic, so a deterministic Rust-side composition is
//! cheaper, more reliable, and easier to test than a sub-agent
//! call.

use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::workflows::ops;
use crate::openhuman::workflows::types::{StateAction, Trigger, WorkflowStateProposal};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct WorkflowProposeEnableTool {
    config: Arc<Config>,
}

impl WorkflowProposeEnableTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WorkflowProposeEnableTool {
    fn name(&self) -> &str {
        super::TOOL_WORKFLOW_PROPOSE_ENABLE
    }

    fn description(&self) -> &str {
        "PREVIEW-ONLY: propose enabling a workflow. Returns a \
         `WorkflowStateProposal { action: Enable, rationale }` payload \
         the UI renders as a click-to-confirm card. This tool does NOT \
         mutate — the user's click triggers `workflows_enable`."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": { "type": "string", "description": "Workflow id to propose enabling." }
            },
            "required": ["workflow_id"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let workflow_id = args
            .get("workflow_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("missing required field `workflow_id`"))?;
        tracing::info!(
            target: "workflows-agent",
            "[workflows-agent] tool=workflow_propose_enable wf={workflow_id}"
        );
        let workflow = match ops::get(&self.config, workflow_id.clone())
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

        let rationale = build_enable_rationale(&workflow);
        let preview = WorkflowStateProposal {
            workflow_id: workflow.id,
            action: StateAction::Enable,
            rationale,
            enabled: !workflow.enabled,
        };
        let payload = json!({ "state_proposal": preview });
        Ok(ToolResult::success(serde_json::to_string(&payload)?))
    }
}

fn build_enable_rationale(workflow: &crate::openhuman::workflows::types::Workflow) -> Vec<String> {
    let mut out = Vec::new();
    if workflow.enabled {
        out.push("Already enabled. Applying will be a no-op.".into());
        return out;
    }
    match &workflow.trigger {
        Trigger::Cron { expr, .. } => {
            out.push(format!("Will resume cron firing on schedule `{expr}`."));
        }
        Trigger::Manual => {
            out.push("Manual trigger — enabling clears the disabled gate; runs still require a manual dispatch.".into());
        }
        Trigger::Webhook { .. }
        | Trigger::ComposioEvent { .. }
        | Trigger::ChannelMessage { .. } => {
            out.push("Will start receiving trigger events.".into());
        }
    }
    out
}
