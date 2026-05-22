//! Stub `channel_send` agent tool — F-8 + Phase 2 deferral.
//!
//! F-8's `build_node_agent_definition` (in
//! `workflows/executor.rs::connection_tool_name`) names this tool when
//! it resolves a `ConnectionRef::Channel` into the agent_prompt
//! sub-agent's allowlist. The actual send-to-channel API doesn't exist
//! in unified form today — each provider (Telegram, Slack, Discord,
//! WhatsApp, …) ships its own outbound `Channel::send`
//! implementation but there's no top-level
//! `send_message_to_channel(provider, channel_id, body)` entry point.
//!
//! Phase 2's F2-5 (`channel_message` node kind) lands the unified send
//! path + this tool's real body. Until then, this stub returns a
//! clear "deferred to Phase 2" error so a workflow execution that
//! reaches this tool fails loud rather than silently — the run row
//! shows the deferral as the failure reason, not an opaque
//! `tool not registered` runtime crash.

use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct ChannelSendStubTool;

impl ChannelSendStubTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ChannelSendStubTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ChannelSendStubTool {
    fn name(&self) -> &str {
        "channel_send"
    }

    fn description(&self) -> &str {
        "PHASE 2 STUB: Send a text message to a connected chat channel \
         (Slack, Discord, Telegram, WhatsApp). The unified send API is \
         a Phase 2 F2-5 deliverable; this tool currently returns a \
         deferred-feature error. For Phase 1, route channel messages \
         through `composio_execute` against a Composio toolkit \
         (Slack/Discord/Telegram via Composio) if connected."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "provider": { "type": "string", "description": "Channel provider (slack/discord/telegram/whatsapp)." },
                "channel_id": { "type": "string", "description": "Resolved channel id." },
                "body": { "type": "string", "description": "Message text." }
            },
            "required": ["provider", "channel_id", "body"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Skill
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        tracing::warn!(
            target: "workflows-agent",
            "[workflows-agent] channel_send stub invoked args={args}; Phase 2 deferred"
        );
        Ok(ToolResult::error(
            "channel_send is a Phase 2 (F2-5) deliverable and isn't wired yet. \
             For Phase 1, the workflow's agent_prompt should route channel \
             messages through `composio_execute` against the matching Composio \
             toolkit (Slack/Discord/Telegram) if the user has it connected."
                .to_string(),
        ))
    }
}
