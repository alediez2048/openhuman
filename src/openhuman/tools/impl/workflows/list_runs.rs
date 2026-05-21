//! `workflows_list_runs` agent tool — paginated runs view for a
//! workflow, newest-first.
//!
//! Read-only per ADR-012 / NFR-2.3.6. The agent calls this to reason
//! about "did the workflow fire?", "what did the last run output?",
//! etc. `limit` is clamped server-side to `[1, 100]` (NFR-2.5.6) so
//! a runaway agent can't request a million-row page.

use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::workflows::ops;
use crate::openhuman::workflows::store::Pagination;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct WorkflowsListRunsTool {
    config: Arc<Config>,
}

impl WorkflowsListRunsTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WorkflowsListRunsTool {
    fn name(&self) -> &str {
        super::TOOL_WORKFLOWS_LIST_RUNS
    }

    fn description(&self) -> &str {
        "List runs for a workflow, newest-first (read-only). `limit` defaults \
         to 50 and is clamped to [1, 100] server-side. Each row carries \
         trigger_source, status, started_at, completed_at, error."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": { "type": "string", "description": "Workflow id." },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                "offset": { "type": "integer", "minimum": 0 }
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

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let workflow_id = args
            .get("workflow_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("missing required field `workflow_id`"))?;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as u32;
        let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as u32;
        let pagination = Pagination { limit, offset };
        tracing::info!(
            target: "workflows-agent",
            "[workflows-agent] tool=workflows_list_runs wf={workflow_id} limit={limit} offset={offset}"
        );
        let outcome = ops::list_runs(&self.config, workflow_id, pagination)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let payload = json!({ "runs": outcome.value });
        Ok(ToolResult::success(serde_json::to_string(&payload)?))
    }
}
