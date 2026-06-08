//! `campaign_propose_resume` — propose-only state transition
//! `Draft | Paused → Active`. Returns the same `<campaign-preview>`
//! preview-card shape as `_pause` / `_archive`.

use crate::openhuman::campaigns::types::{Campaign, CampaignStatus};
use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct CampaignProposeResumeTool {
    config: Arc<Config>,
}

impl CampaignProposeResumeTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for CampaignProposeResumeTool {
    fn name(&self) -> &str {
        super::TOOL_CAMPAIGN_PROPOSE_RESUME
    }

    fn description(&self) -> &str {
        "PREVIEW-ONLY: propose resuming a Draft or Paused campaign → Active. \
         Returns a structured payload + `<campaign-preview>` tag the chat UI \
         renders as Apply/Discard. The user's Apply click triggers \
         `campaigns_resume` which re-enables every child workflow."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Campaign id." }
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

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        super::propose_state::execute(
            &self.config,
            args,
            "resume",
            CampaignStatus::Active,
            hint_for_illegal_resume,
        )
        .await
    }
}

fn hint_for_illegal_resume(c: &Campaign) -> String {
    match c.status {
        CampaignStatus::Active => "Already Active — no transition needed.".into(),
        CampaignStatus::WoundDown => {
            "Campaign is winding down (no new records accepted). To resume \
             accepting new records you'd need a new campaign — wound-down \
             campaigns can only be archived."
                .into()
        }
        CampaignStatus::Archived => "Campaign is Archived (terminal). Cannot be resumed.".into(),
        CampaignStatus::Draft | CampaignStatus::Paused => {
            unreachable!("Draft/Paused → Active is legal — should not hit hint")
        }
    }
}
