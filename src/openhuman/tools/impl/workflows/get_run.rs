//! `workflows_get_run` agent tool — fetch a single run + its
//! persisted step rows. Returns `null` when the id is unknown so the
//! agent can reason "deleted mid-poll vs. wrong id".
//!
//! Read-only per ADR-012 / NFR-2.3.6. The step output is already
//! truncated to 64 KiB on disk (F-8 NFR-2.3.5), so this tool is safe
//! to call without an additional context budget concern.

use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::workflows::ops;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct WorkflowsGetRunTool {
    config: Arc<Config>,
}

impl WorkflowsGetRunTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WorkflowsGetRunTool {
    fn name(&self) -> &str {
        super::TOOL_WORKFLOWS_GET_RUN
    }

    fn description(&self) -> &str {
        "Fetch a single workflow run + its step rows by run_id (read-only). \
         Returns `{run, steps}` or `null` when the id is unknown. Step \
         `output_json` is already truncated to 64 KiB."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "run_id": { "type": "string", "description": "Run id to fetch." }
            },
            "required": ["run_id"],
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
        let run_id = args
            .get("run_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("missing required field `run_id`"))?;
        tracing::info!(
            target: "workflows-agent",
            "[workflows-agent] tool=workflows_get_run run={run_id}"
        );
        let outcome = ops::get_run(&self.config, run_id.clone())
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let payload = json!({ "run_with_steps": outcome.value });
        Ok(ToolResult::success(serde_json::to_string(&payload)?))
    }
}
