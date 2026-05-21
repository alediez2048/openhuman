//! `workflow_get` agent tool — fetch a single workflow by id.
//! Returns the persisted row verbatim, including its `trigger`,
//! `nodes`, `edges`, and computed `health`.
//!
//! Read-only per ADR-012 / NFR-2.3.6. The `Workflow` shape carries
//! no secret material — `ConnectionRef::GenericHttp` references its
//! row by id only; credentials live in `connections.db` and never
//! transit through workflow JSON.

use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::workflows::ops;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct WorkflowGetTool {
    config: Arc<Config>,
}

impl WorkflowGetTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WorkflowGetTool {
    fn name(&self) -> &str {
        super::TOOL_WORKFLOW_GET
    }

    fn description(&self) -> &str {
        "Fetch a single workflow by id (read-only). Returns the full row \
         (trigger, nodes, edges, health, origin). Returns `null` when the \
         id is unknown — distinguish \"deleted mid-poll\" from a transport \
         error this way."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": { "type": "string", "description": "Workflow id to fetch." }
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
        let id = args
            .get("workflow_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("missing required field `workflow_id`"))?;
        tracing::info!(
            target: "workflows-agent",
            "[workflows-agent] tool=workflow_get id={id}"
        );
        let outcome = ops::get(&self.config, id.clone())
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let payload = json!({ "workflow": outcome.value });
        Ok(ToolResult::success(serde_json::to_string(&payload)?))
    }
}
