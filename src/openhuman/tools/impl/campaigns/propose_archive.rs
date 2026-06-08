//! `campaign_propose_archive` — propose-only terminal state
//! transition `WoundDown → Archived`. Returns the same
//! `<campaign-preview>` preview-card shape as `_pause` / `_resume`.

use crate::openhuman::campaigns::types::{Campaign, CampaignStatus};
use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct CampaignProposeArchiveTool {
    config: Arc<Config>,
}

impl CampaignProposeArchiveTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for CampaignProposeArchiveTool {
    fn name(&self) -> &str {
        super::TOOL_CAMPAIGN_PROPOSE_ARCHIVE
    }

    fn description(&self) -> &str {
        "PREVIEW-ONLY: propose archiving a WoundDown campaign — the terminal \
         transition. Archive disables every child workflow regardless of state. \
         Returns a structured payload + `<campaign-preview>` tag the chat UI \
         renders as Apply/Discard. Pre-validates: only WoundDown → Archived is \
         legal (campaigns must be wound down first)."
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
            "archive",
            CampaignStatus::Archived,
            hint_for_illegal_archive,
        )
        .await
    }
}

fn hint_for_illegal_archive(c: &Campaign) -> String {
    match c.status {
        CampaignStatus::Draft | CampaignStatus::Active | CampaignStatus::Paused => {
            "Cannot archive directly — campaigns must be wound down first \
             (use campaign_propose_wind_down). Wind-down lets in-flight \
             conversations finish; archive then terminates fully."
                .into()
        }
        CampaignStatus::Archived => "Already Archived — no transition needed.".into(),
        CampaignStatus::WoundDown => {
            unreachable!("WoundDown → Archived is legal — should not hit hint")
        }
    }
}
