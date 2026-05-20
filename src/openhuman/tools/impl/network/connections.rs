//! Unified Connections Hub agent tool.
//!
//! The agent previously only had `composio_list_connections`, which made it
//! think "what's connected" = "what's in Composio". After Phase 0 (the
//! Connections Hub) the truth is broader: Composio toolkits AND chat
//! channels AND CEF browser accounts AND MCP servers AND user-saved Generic
//! HTTP endpoints AND backend-proxied built-in integrations all count. This
//! tool gives the agent a single read against the unified aggregator that
//! powers the `/connections` page — same source of truth, same view.
//!
//! Returns one row per connection, grouped by mechanism. Status reflects
//! the aggregator's view (Connected / NotConnected / Error / Disabled) plus
//! the last verification probe outcome when available.
//!
//! The agent should call this **before** any reasoning about "do I have X
//! connected?" — never assume the Composio-only listing is exhaustive.

use crate::openhuman::config::Config;
use crate::openhuman::connections::aggregator;
use crate::openhuman::connections::types::{ConnectionKind, ConnectionRef, ConnectionStatus};
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct ConnectionsListTool {
    config: Arc<Config>,
}

impl ConnectionsListTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for ConnectionsListTool {
    fn name(&self) -> &str {
        "list_connections"
    }

    fn description(&self) -> &str {
        "List **every** connection across all 6 OpenHuman connection categories \
         (Composio toolkits, chat channels, CEF browser accounts, built-in \
         integrations, MCP servers, user-saved Generic HTTP endpoints). Use this \
         as the authoritative answer to \"what's connected?\" — composio_list_connections \
         only covers Composio. Each row carries {ref, display_name, status, mechanism, \
         verification}. Verification is the result of the last real probe (Live / \
         Failed / null=never tested); status is the mechanism's own view (DB row \
         exists, cookies present, etc.) and is weaker evidence."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false,
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Skill
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        tracing::debug!("[connections] tool list_connections.execute");
        let rows = aggregator::list_all(&self.config).await?;

        // Build a markdown table grouped by mechanism so the agent can scan
        // the answer without reparsing JSON.
        let mut by_mech: std::collections::BTreeMap<&str, Vec<&_>> =
            std::collections::BTreeMap::new();
        for r in &rows {
            by_mech
                .entry(mechanism_label(&r.r#ref))
                .or_default()
                .push(r);
        }

        let mut md = String::from("# Connections\n\n");
        for (label, group) in &by_mech {
            md.push_str(&format!("## {label} ({})\n\n", group.len()));
            for row in group {
                let status_label = match &row.status {
                    ConnectionStatus::Connected => "Connected",
                    ConnectionStatus::NotConnected => "Not connected",
                    ConnectionStatus::Disabled => "Disabled",
                    ConnectionStatus::Error { reason } => {
                        md.push_str(&format!("- **{}** — Error: {reason}\n", row.display_name));
                        continue;
                    }
                };
                let verification_label = match &row.verification {
                    None => String::new(),
                    Some(v) => match &v.result {
                        crate::openhuman::connections::verification::VerificationResult::Live => {
                            format!(" · Verified {}", v.last_probed_at.format("%Y-%m-%d %H:%M"))
                        }
                        crate::openhuman::connections::verification::VerificationResult::Failed {
                            reason,
                        } => format!(" · Probe failed: {reason}"),
                    },
                };
                md.push_str(&format!(
                    "- **{}** — {status_label}{verification_label}\n",
                    row.display_name
                ));
            }
            md.push('\n');
        }
        if rows.is_empty() {
            md.push_str("_No connections yet._\n");
        }

        // JSON shape kept compact + machine-readable; pairs the human
        // markdown so the agent can either pretty-print or reason
        // structurally.
        let connections_json = serde_json::to_value(&rows)?;
        Ok(ToolResult::success_with_markdown(
            json!({ "connections": connections_json }),
            md,
        ))
    }
}

fn mechanism_label(r: &ConnectionRef) -> &'static str {
    match ConnectionKind::from_ref(r) {
        ConnectionKind::Composio => "Composio",
        ConnectionKind::Channel => "Chat Channels",
        ConnectionKind::Webview => "Browser Accounts",
        ConnectionKind::Builtin => "Built-in Integrations",
        ConnectionKind::Mcp => "MCP Servers",
        ConnectionKind::GenericHttp => "Generic HTTP",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::config::Config;
    use tempfile::TempDir;

    fn fake_config() -> (TempDir, Arc<Config>) {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        (dir, Arc::new(config))
    }

    #[tokio::test]
    async fn list_connections_returns_markdown_grouped_by_mechanism() {
        let (_dir, config) = fake_config();
        let tool = ConnectionsListTool::new(config);
        let result = tool.execute(json!({})).await.unwrap();
        let rendered = result
            .markdown_formatted
            .as_deref()
            .expect("tool result should carry markdown");

        // Fresh workspace has built-in + MCP (gitbooks) + channels +
        // webview baseline. The markdown should mention each category that
        // has any rows.
        assert!(rendered.contains("# Connections"));
        assert!(rendered.contains("## Built-in Integrations"));
        assert!(rendered.contains("## MCP Servers"));
    }

    #[tokio::test]
    async fn list_connections_includes_all_six_categories_when_present() {
        // Sanity that mechanism_label maps every ConnectionKind. If a new
        // mechanism is added without a label arm, this test catches it.
        for kind in [
            ConnectionKind::Composio,
            ConnectionKind::Channel,
            ConnectionKind::Webview,
            ConnectionKind::Builtin,
            ConnectionKind::Mcp,
            ConnectionKind::GenericHttp,
        ] {
            let r#ref = match kind {
                ConnectionKind::Composio => ConnectionRef::Composio {
                    toolkit_id: "x".into(),
                    account_id: None,
                },
                ConnectionKind::Channel => ConnectionRef::Channel {
                    provider: "x".into(),
                    channel_id: "x".into(),
                },
                ConnectionKind::Webview => ConnectionRef::Webview {
                    provider: "x".into(),
                    account_id: "x".into(),
                },
                ConnectionKind::Builtin => ConnectionRef::Builtin {
                    integration: "x".into(),
                },
                ConnectionKind::Mcp => ConnectionRef::Mcp {
                    server_id: "x".into(),
                    tool_name: None,
                },
                ConnectionKind::GenericHttp => ConnectionRef::GenericHttp {
                    connection_id: "x".into(),
                },
            };
            assert!(!mechanism_label(&r#ref).is_empty());
        }
    }
}
