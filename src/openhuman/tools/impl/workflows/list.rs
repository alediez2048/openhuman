//! `workflow_list` agent tool — read-only listing of the user's
//! workflows. Wraps the same `ops::list` path the `workflows_list`
//! RPC uses, so the agent sees identical rows to the
//! `/workflows` UI.
//!
//! Read-only per ADR-012 / NFR-2.3.6: no mutation, no side effects,
//! no secret material in the response shape (`Workflow` doesn't
//! transitively carry credentials — `ConnectionRef::GenericHttp` is
//! a `connection_id` only).

use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::workflows::ops;
use crate::openhuman::workflows::types::ListFilter;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct WorkflowListTool {
    config: Arc<Config>,
}

impl WorkflowListTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WorkflowListTool {
    fn name(&self) -> &str {
        super::TOOL_WORKFLOW_LIST
    }

    fn description(&self) -> &str {
        "List the user's workflows (read-only). Returns each workflow's id, name, \
         health, enabled bit, trigger, and origin. Optional `filter` accepts \
         `{enabled?: bool, health_state?: \"ready|needs_connections|last_run_failed|session_expired\", \
         search?: string}`. Use this before reasoning about \"do I already have a workflow that …\"."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filter": {
                    "type": "object",
                    "properties": {
                        "enabled": { "type": "boolean" },
                        "health_state": {
                            "type": "string",
                            "enum": ["ready", "needs_connections", "last_run_failed", "session_expired"]
                        },
                        "search": { "type": "string" }
                    },
                    "additionalProperties": false
                }
            },
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
        let filter: ListFilter = match args.get("filter") {
            Some(v) if !v.is_null() => serde_json::from_value(v.clone())
                .map_err(|e| anyhow::anyhow!("invalid `filter`: {e}"))?,
            _ => ListFilter::default(),
        };
        tracing::info!(
            target: "workflows-agent",
            "[workflows-agent] tool=workflow_list filter={filter:?}"
        );
        let outcome = ops::list(&self.config, filter)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let payload = json!({ "workflows": outcome.value });
        Ok(ToolResult::success(serde_json::to_string(&payload)?))
    }
}
