//! `workflow_propose_disable` — propose-only disable preview (F-12).
//!
//! Mirrors [`super::propose_enable`]: returns a
//! [`WorkflowStateProposal { action: Disable, rationale }`] payload
//! the UI renders as a click-to-confirm card. Does NOT call
//! `workflows_disable` — ADR-012's single-mutation-boundary contract.

use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::workflows::ops;
use crate::openhuman::workflows::types::{StateAction, Trigger, WorkflowStateProposal};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct WorkflowProposeDisableTool {
    config: Arc<Config>,
}

impl WorkflowProposeDisableTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WorkflowProposeDisableTool {
    fn name(&self) -> &str {
        super::TOOL_WORKFLOW_PROPOSE_DISABLE
    }

    fn description(&self) -> &str {
        "PREVIEW-ONLY: propose disabling a workflow. Returns a \
         `WorkflowStateProposal { action: Disable, rationale }` payload \
         the UI renders as a click-to-confirm card. This tool does NOT \
         mutate."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": { "type": "string", "description": "Workflow id to propose disabling." }
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

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let workflow_id = args
            .get("workflow_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("missing required field `workflow_id`"))?;
        tracing::info!(
            target: "workflows-agent",
            "[workflows-agent] tool=workflow_propose_disable wf={workflow_id}"
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

        let rationale = if !workflow.enabled {
            vec!["Already disabled. Applying will be a no-op.".into()]
        } else {
            match &workflow.trigger {
                Trigger::Cron { .. } => {
                    vec![
                        "Will stop cron firing immediately. Any in-flight run finishes naturally."
                            .into(),
                    ]
                }
                Trigger::Manual => {
                    vec!["Manual trigger — disabling blocks future manual dispatches.".into()]
                }
                _ => vec!["Will stop receiving trigger events.".into()],
            }
        };
        let preview = WorkflowStateProposal {
            workflow_id: workflow.id,
            action: StateAction::Disable,
            rationale,
            enabled: workflow.enabled,
        };
        let json_str = serde_json::to_string(&preview)?;
        let data_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            json_str.as_bytes(),
        );
        let preview_tag =
            format!("<workflow-preview kind=\"state\" data=\"{data_b64}\"></workflow-preview>");
        let payload = json!({
            "status": "state_preview_ready",
            "render_instructions": "Include the `preview_tag` value verbatim in your user-facing reply. Do not call workflow_propose_disable again — the user clicks Apply on the rendered card.",
            "preview_tag": preview_tag,
            "state_proposal": preview,
        });
        let markdown = format!(
            "Disable preview ready. Include this tag verbatim in your reply, then stop — do NOT call this tool again.\n\n{preview_tag}"
        );
        Ok(ToolResult::success_with_markdown(payload, markdown))
    }
}
