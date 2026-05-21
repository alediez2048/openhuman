//! `workflow_propose_delete` — propose-only delete preview (F-12).
//!
//! Returns a [`WorkflowDeletePreview { workflow_id, name, run_count,
//! retention_days: 30 }`] payload the F-14 UI renders as a "delete
//! workflow X with N runs?" confirmation. The cascade-delete itself
//! happens only when the user clicks Apply, which triggers the
//! `workflows_delete` RPC (ADR-012).
//!
//! `retention_days` is hard-coded to 30 per FR-1.3.4. Phase 1
//! ships the lean default; an explicit retention-tunable lands in
//! Phase 2 if user research demands it.

use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::workflows::ops;
use crate::openhuman::workflows::types::WorkflowDeletePreview;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// Phase 1 retention window before a soft-deleted workflow's history
/// is GC'd. Documented in `WorkflowDeletePreview::retention_days`
/// per FR-1.3.4; the cascade delete (F-2's `workflows_delete`) is
/// hard today and the retention sweep is deferred to F-15.
const RETENTION_DAYS: u32 = 30;

pub struct WorkflowProposeDeleteTool {
    config: Arc<Config>,
}

impl WorkflowProposeDeleteTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WorkflowProposeDeleteTool {
    fn name(&self) -> &str {
        super::TOOL_WORKFLOW_PROPOSE_DELETE
    }

    fn description(&self) -> &str {
        "PREVIEW-ONLY: propose deleting a workflow. Returns a \
         `WorkflowDeletePreview { workflow_id, name, run_count, \
         retention_days: 30 }` payload the UI renders as a \
         click-to-confirm card. This tool does NOT mutate."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": { "type": "string", "description": "Workflow id to propose deleting." }
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
            "[workflows-agent] tool=workflow_propose_delete wf={workflow_id}"
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
        let run_count = ops::count_runs(&self.config, &workflow.id)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .value;
        let preview = WorkflowDeletePreview {
            workflow_id: workflow.id,
            name: workflow.name,
            run_count,
            retention_days: RETENTION_DAYS,
        };
        let payload = json!({ "delete_preview": preview });
        Ok(ToolResult::success(serde_json::to_string(&payload)?))
    }
}
