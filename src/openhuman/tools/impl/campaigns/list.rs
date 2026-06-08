//! `campaign_list` agent tool — read-only listing of the user's
//! campaigns. Wraps `campaigns::ops::list` so the agent sees identical
//! rows to the `/campaigns` UI route.

use crate::openhuman::campaigns::ops;
use crate::openhuman::campaigns::store::ListCampaignsFilter;
use crate::openhuman::campaigns::types::CampaignStatus;
use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct CampaignListTool {
    config: Arc<Config>,
}

impl CampaignListTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for CampaignListTool {
    fn name(&self) -> &str {
        super::TOOL_CAMPAIGN_LIST
    }

    fn description(&self) -> &str {
        "List the user's campaigns (read-only). Returns each campaign's id, name, \
         status, entity_binding, throttle, approval_policy, and target_outcome. \
         Optional `filter` accepts `{status?: \"draft|active|paused|wound_down|archived\", \
         include_deleted?: bool}`. Use this before reasoning about \"do I already \
         have a campaign for …\"."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filter": {
                    "type": "object",
                    "properties": {
                        "status": {
                            "type": "string",
                            "enum": ["draft", "active", "paused", "wound_down", "archived"]
                        },
                        "include_deleted": { "type": "boolean" }
                    }
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

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let filter = parse_filter(args.get("filter"));
        match ops::list(&self.config, filter).await {
            Ok(outcome) => {
                let body = serde_json::to_string(&json!({
                    "campaigns": outcome.value,
                }))?;
                Ok(ToolResult::success(body))
            }
            Err(err) => Ok(ToolResult::error(format!("campaign_list failed: {err}"))),
        }
    }
}

fn parse_filter(v: Option<&Value>) -> ListCampaignsFilter {
    let Some(obj) = v.and_then(|v| v.as_object()) else {
        return ListCampaignsFilter::default();
    };
    let status = obj
        .get("status")
        .and_then(Value::as_str)
        .and_then(|s| match s {
            "draft" => Some(CampaignStatus::Draft),
            "active" => Some(CampaignStatus::Active),
            "paused" => Some(CampaignStatus::Paused),
            "wound_down" => Some(CampaignStatus::WoundDown),
            "archived" => Some(CampaignStatus::Archived),
            _ => None,
        });
    let include_deleted = obj
        .get("include_deleted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    ListCampaignsFilter {
        status,
        include_deleted,
    }
}
