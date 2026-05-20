//! MCP bridge tools for the agent.
//!
//! All three tools (`mcp_list_servers`, `mcp_list_tools`, `mcp_call_tool`)
//! **rebuild the `McpServerRegistry` from `Config` on every `execute()`**
//! rather than holding an `Arc<McpServerRegistry>` snapshot taken at
//! agent boot.
//!
//! Why: the user adds MCP servers via the in-app modal
//! (`connections_mcp_add` mutates `config.mcp_client.servers` and saves
//! the TOML). The aggregator's `collect_mcp` already rebuilds the
//! registry per call, so the Hub picks up new servers without a core
//! restart. The agent's tools used to hold a snapshot Arc, which made
//! new servers invisible mid-session — the user-reported "no higgsfield
//! mcp in the connected integrations" bug.

use crate::openhuman::config::Config;
use crate::openhuman::mcp_client::{McpRegistrySource, McpServerRegistry};
use crate::openhuman::security::{SecurityPolicy, ToolOperation};
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCallOptions, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// Helper: build a fresh registry from the current Config. Used by every
/// MCP tool's `execute()` so a server added via `connections_mcp_add` mid-
/// session is immediately visible without a core restart.
fn fresh_registry(config: &Config) -> McpServerRegistry {
    McpServerRegistry::from_config(config)
}

pub struct McpListServersTool {
    config: Arc<Config>,
}

impl McpListServersTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for McpListServersTool {
    fn name(&self) -> &str {
        "mcp_list_servers"
    }

    fn description(&self) -> &str {
        "List named remote MCP servers registered in OpenHuman core. Use this before browsing tools on a specific MCP server. Reads the current `config.mcp_client.servers` on every call, so servers added via the Connections Hub mid-session are immediately visible."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        let registry = fresh_registry(&self.config);
        let registered: Vec<_> = registry.list().iter().map(|s| (*s).clone()).collect();
        tracing::debug!(
            target: "mcp_client",
            count = registered.len(),
            "[mcp] mcp_list_servers — fresh registry built from current config"
        );
        let servers = registered
            .iter()
            .map(|server| {
                json!({
                    "name": server.name,
                    "endpoint": server.endpoint,
                    "description": server.description,
                    "timeout_secs": server.timeout_secs,
                    "auth": server.auth,
                    "source": server.source,
                })
            })
            .collect::<Vec<_>>();

        let markdown = if registered.is_empty() {
            "# MCP Servers\n\nNo remote MCP servers are registered.".to_string()
        } else {
            let mut md = String::from("# MCP Servers\n");
            for server in &registered {
                let source = match server.source {
                    McpRegistrySource::Config => "config",
                    McpRegistrySource::LegacyGitbooks => "legacy_gitbooks",
                };
                md.push_str(&format!(
                    "\n- **{}** ({source})\n  - endpoint: `{}`\n  - auth: `{}`",
                    server.name,
                    server.endpoint,
                    match &server.auth {
                        crate::openhuman::config::McpAuthConfig::None => "none",
                        crate::openhuman::config::McpAuthConfig::BearerToken { .. } =>
                            "bearer_token",
                        crate::openhuman::config::McpAuthConfig::Basic { .. } => "basic",
                        crate::openhuman::config::McpAuthConfig::Header { .. } => "header",
                        crate::openhuman::config::McpAuthConfig::QueryParam { .. } => "query_param",
                    }
                ));
                if let Some(description) = server.description.as_deref() {
                    md.push_str(&format!("\n  - {description}"));
                }
            }
            md
        };

        Ok(ToolResult::success_with_markdown(
            json!({ "servers": servers }),
            markdown,
        ))
    }
}

pub struct McpListToolsTool {
    config: Arc<Config>,
}

impl McpListToolsTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for McpListToolsTool {
    fn name(&self) -> &str {
        "mcp_list_tools"
    }

    fn description(&self) -> &str {
        "List tools exposed by a named remote MCP server. Use this before calling `mcp_call_tool`."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Registered MCP server name from `mcp_list_servers`."
                }
            },
            "required": ["server"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let server = required_string_arg(&args, "server")?;
        let registry = fresh_registry(&self.config);
        let tools = match registry.list_tools(&server).await {
            Ok(tools) => tools,
            Err(err) => return Ok(ToolResult::error(format!("mcp_list_tools failed: {err}"))),
        };

        let payload = tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "title": tool.title,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                })
            })
            .collect::<Vec<_>>();

        let mut markdown = format!("# MCP Tools: `{server}`\n");
        if tools.is_empty() {
            markdown.push_str("\nNo tools were returned by the remote server.");
        } else {
            for tool in &tools {
                markdown.push_str(&format!(
                    "\n- **{}**: {}\n  - schema: `{}`",
                    tool.name,
                    tool.description.as_deref().unwrap_or("No description."),
                    serde_json::to_string(&tool.input_schema).unwrap_or_else(|_| "{}".into())
                ));
            }
        }

        Ok(ToolResult::success_with_markdown(
            json!({ "server": server, "tools": payload }),
            markdown,
        ))
    }
}

pub struct McpCallTool {
    config: Arc<Config>,
    security: Arc<SecurityPolicy>,
}

impl McpCallTool {
    pub fn new(config: Arc<Config>, security: Arc<SecurityPolicy>) -> Self {
        Self { config, security }
    }
}

#[async_trait]
impl Tool for McpCallTool {
    fn name(&self) -> &str {
        "mcp_call_tool"
    }

    fn description(&self) -> &str {
        "Call a tool on a named remote MCP server. First inspect available tools with `mcp_list_tools`, then pass the remote tool name and its JSON arguments here."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Registered MCP server name from `mcp_list_servers`."
                },
                "tool": {
                    "type": "string",
                    "description": "Remote MCP tool name from `mcp_list_tools`."
                },
                "arguments": {
                    "type": "object",
                    "description": "Arguments object passed through to the remote MCP tool."
                }
            },
            "required": ["server", "tool", "arguments"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute_with_options(
        &self,
        args: Value,
        options: ToolCallOptions,
    ) -> anyhow::Result<ToolResult> {
        self.security
            .enforce_tool_operation(ToolOperation::Act, self.name())
            .map_err(|err| anyhow::anyhow!(err))?;

        let server = required_string_arg(&args, "server")?;
        let tool = required_string_arg(&args, "tool")?;
        let arguments = args
            .get("arguments")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing required `arguments` object"))?;
        if !arguments.is_object() {
            return Ok(ToolResult::error("`arguments` must be an object"));
        }

        let registry = fresh_registry(&self.config);
        let mut result = match registry.call_tool(&server, &tool, arguments).await {
            Ok(result) => result.rendered,
            Err(err) => return Ok(ToolResult::error(format!("mcp_call_tool failed: {err}"))),
        };

        if options.prefer_markdown && result.markdown_formatted.is_none() {
            result.markdown_formatted = Some(result.output());
        }
        Ok(result)
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_with_options(args, ToolCallOptions::default())
            .await
    }
}

fn required_string_arg(args: &Value, key: &str) -> anyhow::Result<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("missing required `{key}` argument"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::config::McpServerConfig;
    use tempfile::TempDir;

    fn config_with(servers: Vec<McpServerConfig>) -> (TempDir, Arc<Config>) {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        config.mcp_client.servers = servers;
        (dir, Arc::new(config))
    }

    #[tokio::test]
    async fn list_servers_includes_user_registered_entries() {
        // Regression for the user-reported "no higgsfield mcp" bug: the
        // tool previously held an Arc<McpServerRegistry> snapshot, so
        // servers added via `connections_mcp_add` after tool construction
        // were invisible. Now the tool reads Config on every call.
        //
        // The default Config auto-registers the legacy `gitbooks` server,
        // so the baseline isn't empty — the contract this test guards is
        // that a *newly-added* server surfaces alongside it.
        let (_dir, config) = config_with(vec![McpServerConfig {
            name: "higgsfield".into(),
            endpoint: "https://mcp.example.com".into(),
            enabled: true,
            ..Default::default()
        }]);
        let tool = McpListServersTool::new(Arc::clone(&config));
        let result = tool.execute(json!({})).await.unwrap();
        let md = result.markdown_formatted.as_deref().unwrap();
        assert!(
            md.contains("higgsfield"),
            "server added in config.mcp_client.servers must surface; got:\n{md}"
        );
    }
}
