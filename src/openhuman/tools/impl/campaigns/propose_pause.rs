//! `campaign_propose_pause` — propose-only state transition.
//!
//! Returns a `<campaign-preview kind="state" action="pause">` tag the
//! chat-runtime parses + the UI renders as a "Pause campaign X?"
//! Apply/Discard card. ADR-012: the user's Apply click is the single
//! mutation boundary; this tool does NOT mutate.

use crate::openhuman::campaigns::types::{Campaign, CampaignStatus};
use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct CampaignProposePauseTool {
    config: Arc<Config>,
}

impl CampaignProposePauseTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for CampaignProposePauseTool {
    fn name(&self) -> &str {
        super::TOOL_CAMPAIGN_PROPOSE_PAUSE
    }

    fn description(&self) -> &str {
        "PREVIEW-ONLY: propose pausing a campaign. Returns a structured \
         payload + `<campaign-preview>` tag the chat UI renders as an \
         Apply/Discard card. Validates Active → Paused; returns an \
         `invalid_transition` error payload when the current status doesn't \
         allow the move. The user's Apply click triggers `campaigns_pause`."
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
            "pause",
            CampaignStatus::Paused,
            hint_for_illegal_pause,
        )
        .await
    }
}

fn hint_for_illegal_pause(c: &Campaign) -> String {
    match c.status {
        CampaignStatus::Draft => {
            "Pause requires an Active campaign. Draft campaigns haven't started — \
             use campaign_propose_resume to start it first, OR delete it instead."
                .into()
        }
        CampaignStatus::Paused => "Already Paused — no transition needed.".into(),
        CampaignStatus::WoundDown => {
            "Campaign is already winding down (no new records accepted). Use \
             campaign_propose_archive to terminate fully."
                .into()
        }
        CampaignStatus::Archived => {
            "Campaign is Archived (terminal). Cannot be paused — restore the \
             campaign first if you want to re-activate."
                .into()
        }
        CampaignStatus::Active => unreachable!("Active → Paused is legal — should not hit hint"),
    }
}
