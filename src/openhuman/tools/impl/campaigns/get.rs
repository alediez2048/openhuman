//! `campaign_get` agent tool — read-only single-row fetch.

use crate::openhuman::campaigns::ops;
use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct CampaignGetTool {
    config: Arc<Config>,
}

impl CampaignGetTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for CampaignGetTool {
    fn name(&self) -> &str {
        super::TOOL_CAMPAIGN_GET
    }

    fn description(&self) -> &str {
        "Fetch a single campaign by id (read-only). Returns the full Campaign \
         row, or `{ campaign: null }` when the id is unknown / soft-deleted. \
         Use after `campaign_list` to drill into a specific campaign's \
         throttle, approval_policy, or entity_binding."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Campaign id (UUIDv4)." }
            },
            "required": ["id"],
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
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("missing required field `id`"))?;
        match ops::get(&self.config, id).await {
            Ok(outcome) => {
                let body = serde_json::to_string(&json!({
                    "campaign": outcome.value,
                }))?;
                Ok(ToolResult::success(body))
            }
            Err(err) => Ok(ToolResult::error(format!("campaign_get failed: {err}"))),
        }
    }
}
