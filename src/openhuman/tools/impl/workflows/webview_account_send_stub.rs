//! Stub `webview_account_send` agent tool — F-8 + Phase 2 deferral.
//!
//! F-8's `build_node_agent_definition` names this tool when it
//! resolves a `ConnectionRef::Webview` into the agent_prompt
//! sub-agent's allowlist. The webview-account domain today is
//! login-detection only (probes CEF cookies); there is no public
//! outbound send API for any provider (LinkedIn, Telegram, WhatsApp,
//! Slack via webview, …).
//!
//! Phase 2's F2-5 needs to either land webview outbound or document
//! that webview-only providers can't be sender nodes. Until then,
//! this stub returns a clear "deferred" error so workflow runs
//! that hit the tool fail loud with a clear reason.

use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct WebviewAccountSendStubTool;

impl WebviewAccountSendStubTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebviewAccountSendStubTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebviewAccountSendStubTool {
    fn name(&self) -> &str {
        "webview_account_send"
    }

    fn description(&self) -> &str {
        "PHASE 2 STUB: Send a message via a CEF webview account session \
         (LinkedIn, Telegram-web, WhatsApp-web). The webview-accounts \
         domain is login-detection only today; outbound send is a \
         Phase 2 F2-5 deliverable. This tool currently returns a \
         deferred-feature error. For Phase 1, use a Composio-routed \
         channel if available, or read-only workflows that scrape \
         from the webview without sending."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "provider": { "type": "string", "description": "Webview provider (linkedin/telegram/whatsapp/...)." },
                "account_id": { "type": "string", "description": "Resolved account id." },
                "body": { "type": "string", "description": "Message text." }
            },
            "required": ["provider", "account_id", "body"]
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
            "[workflows-agent] webview_account_send stub invoked args={args}; Phase 2 deferred"
        );
        Ok(ToolResult::error(
            "webview_account_send is a Phase 2 (F2-5) deliverable and isn't \
             wired yet. The webview-accounts domain today only handles \
             login detection; no outbound send API exists. For Phase 1, \
             prefer Composio-routed channels, or workflows that read from \
             webview accounts without sending."
                .to_string(),
        ))
    }
}
