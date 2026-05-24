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

/// F-19 Part 1: structured kinds of MCP tool-call failure. Extended
/// after the post-implementation audit revealed `mcp_list_tools` was
/// the actual call path the chat orchestrator uses (not the original
/// `mcp_call_tool` instrumentation target) — and that Higgsfield's
/// server has a session-propagation race that surfaces as transient
/// 404 "Session not found", distinct from a wrong-path 404.
///
/// Pre-F-19 the `Err(err)` path in `McpCallTool::execute_with_options`
/// formatted the anyhow error as a free-form string ("mcp_call_tool
/// failed: {err}") and handed it to the orchestrator LLM. The LLM
/// pattern-matched on the surrounding chat and invented plausible-
/// sounding root causes ("HTTP 401 from Higgsfield, your token is
/// invalid") even when the real error was a path mismatch — this is
/// the F-19 confabulation bug.
///
/// F-19 classifies the underlying error string into one of these
/// kinds, renders a deterministic message with a stable shape, and
/// teaches the orchestrator prompt to surface that message verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpToolErrorKind {
    /// Server name didn't resolve to a registered MCP server.
    UnknownServer,
    /// HTTP 401 / 403 from the MCP endpoint.
    AuthFailed,
    /// HTTP 404 from the MCP endpoint (wrong path or server not running).
    EndpointNotFound,
    /// HTTP 5xx from the MCP endpoint.
    ServerError,
    /// DNS / TLS / connection-refused. Endpoint unreachable below the
    /// HTTP layer.
    EndpointUnreachable,
    /// Tool exists on the server but server returned an application-
    /// level JSON-RPC error (e.g. invalid arguments).
    ToolReturnedError,
    /// Request body / response framing didn't match MCP shape (e.g.
    /// got HTML back, JSON-RPC `result` missing).
    ProtocolMismatch,
    /// Catch-all for cases the classifier doesn't recognise. Carries
    /// the raw detail through but flags it explicitly as "unknown" so
    /// the orchestrator doesn't guess.
    Unknown,
    /// 404 with a body indicating the upstream MCP server's session
    /// state was lost / not yet replicated (e.g. Higgsfield's
    /// `"Session not found"` 404 between `initialize` and the
    /// immediately-following `tools/list`). The client's internal
    /// reset+retry handles transient cases; this kind surfaces when
    /// it bubbles up so the orchestrator knows it's NOT a wrong-path
    /// 404 — agent-level retry (re-call the same tool) usually works.
    TransientSessionLost,
}

impl McpToolErrorKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::UnknownServer => "unknown_server",
            Self::AuthFailed => "auth_failed",
            Self::EndpointNotFound => "endpoint_not_found",
            Self::ServerError => "server_error",
            Self::EndpointUnreachable => "endpoint_unreachable",
            Self::ToolReturnedError => "tool_returned_error",
            Self::ProtocolMismatch => "protocol_mismatch",
            Self::Unknown => "unknown",
            Self::TransientSessionLost => "transient_session_lost",
        }
    }

    /// Suggestion text appended to the structured error so the user
    /// has an actionable next step. The orchestrator surfaces this
    /// verbatim instead of inventing remediation prose.
    pub(crate) fn suggestion(self) -> &'static str {
        match self {
            Self::UnknownServer => {
                "Check the server name. Use `mcp_list_servers` to see what's registered."
            }
            Self::AuthFailed => {
                "Generate a fresh API token from the upstream provider and update Settings → Connections → MCP."
            }
            Self::EndpointNotFound => {
                "The endpoint path is likely wrong. Try /mcp, /sse, or /messages — every server is different. F-19's auto-probe will fix this on the next Add MCP."
            }
            Self::ServerError => {
                "Upstream MCP server returned a 5xx — likely transient. Try again in a minute, or check the server's status page."
            }
            Self::EndpointUnreachable => {
                "Couldn't reach the server. Check the URL spelling, your network, and whether the server is online."
            }
            Self::ToolReturnedError => {
                "The server received the request and rejected it at the application level. The detail text contains the server's reason."
            }
            Self::ProtocolMismatch => {
                "The endpoint responded but the body wasn't valid MCP. The URL may point at a non-MCP service, or the server speaks a different protocol version."
            }
            Self::Unknown => {
                "Inspect the detail below. Do NOT invent a root cause; ask the user to check their server config or share the runtime log."
            }
            Self::TransientSessionLost => {
                "Upstream MCP server lost its session state (race between `initialize` and the next call — known issue on Higgsfield and similar serverless MCP hosts). The client's internal retry didn't catch it; just call the same tool again in 1-2 seconds and it will succeed."
            }
        }
    }
}

/// Classify an opaque anyhow / error string from the MCP client into a
/// structured kind. Conservative: when no pattern matches, returns
/// `Unknown` instead of guessing — the orchestrator prompt teaches the
/// LLM to surface "unknown" plainly rather than invent details.
///
/// Matches the error strings emitted by `mcp_client::client.rs`:
///   - `"MCP unauthorized for `..` (HTTP 401..."` (line ~803)
///   - `"MCP HTTP 404 — ..."` / `"MCP HTTP <status> — ..."` (line ~809)
///   - `"MCP events GET <status> — ..."` (line ~353)
///   - `"MCP DELETE failed with <status>"` (line ~372)
///   - `"Failed to parse MCP JSON response: ..."` (line ~816)
///   - `"No SSE data frame found in MCP response: ..."` (line ~658)
///   - generic reqwest send errors (DNS / TLS / refused)
pub(crate) fn classify_mcp_error(err_string: &str) -> McpToolErrorKind {
    let lower = err_string.to_lowercase();
    if lower.contains("unauthorized") || lower.contains("http 401") || lower.contains("http 403") {
        return McpToolErrorKind::AuthFailed;
    }
    // Specific 404 sub-case BEFORE the generic 404 catch: Higgsfield (and
    // similar serverless MCP hosts) sometimes return 404 with body
    // `"Session not found"` between `initialize` and an immediately
    // following tool call — race between session creation and propagation.
    // This is transient + agent-level retry usually works; it is NOT a
    // wrong-path 404. Misclassifying it as `EndpointNotFound` would
    // misadvise the user to change their endpoint path when the path is
    // actually correct.
    if lower.contains("session not found")
        || lower.contains("session expired")
        || lower.contains("invalid session")
    {
        return McpToolErrorKind::TransientSessionLost;
    }
    if lower.contains("http 404") || lower.contains("not found") {
        return McpToolErrorKind::EndpointNotFound;
    }
    if lower.contains("http 5") || lower.contains("server error") || lower.contains("bad gateway") {
        return McpToolErrorKind::ServerError;
    }
    if lower.contains("dns error")
        || lower.contains("connection refused")
        || lower.contains("connect error")
        || lower.contains("tcp connect")
        || lower.contains("error sending request")
        || lower.contains("name or service not known")
        || lower.contains("nodename nor servname provided")
    {
        return McpToolErrorKind::EndpointUnreachable;
    }
    if lower.contains("failed to parse mcp json")
        || lower.contains("no sse data frame")
        || lower.contains("parsing initialize result")
        || lower.contains("protocol version")
    {
        return McpToolErrorKind::ProtocolMismatch;
    }
    if lower.contains("unknown server")
        || lower.contains("server not found")
        || lower.contains("server `")
            && lower.contains("not registered")
    {
        return McpToolErrorKind::UnknownServer;
    }
    if lower.contains("tool error") || lower.contains("isError") || lower.contains("jsonrpc error")
    {
        return McpToolErrorKind::ToolReturnedError;
    }
    McpToolErrorKind::Unknown
}

/// Render the F-19 stable error format. The orchestrator prompt
/// (`agent/agents/orchestrator/prompt.md`) is taught to surface this
/// verbatim — preserving the leading `⚠ MCP tool error` marker and
/// the labeled lines — instead of paraphrasing or inventing details.
pub(crate) fn render_mcp_tool_error(
    server: &str,
    tool: &str,
    kind: McpToolErrorKind,
    detail: &str,
) -> String {
    format!(
        "⚠ MCP tool error\nserver: {server}\ntool: {tool}\nkind: {}\ndetail: {detail}\nsuggestion: {}\n\n[Surface this block verbatim. Do NOT invent additional error details.]",
        kind.label(),
        kind.suggestion()
    )
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
            Err(err) => {
                // F-19 Part 1 (extended after the agent-audit revealed
                // this was the actual call path the chat orchestrator
                // uses — NOT `mcp_call_tool`). Classify the error and
                // surface as the structured `⚠ MCP tool error` block
                // the orchestrator prompt teaches the LLM to render
                // verbatim. Pre-fix, this branch returned a raw
                // `anyhow!` string the LLM confabulated into invented
                // HTTP status codes ("401 Invalid or expired token"
                // for a 404 "Session not found", etc.).
                let detail = err.to_string();
                let kind = classify_mcp_error(&detail);
                tracing::warn!(
                    target: "mcp",
                    server = %server,
                    tool = "list_tools",
                    kind = %kind.label(),
                    "[mcp_list_tools] failed: {detail}"
                );
                return Ok(ToolResult::error(render_mcp_tool_error(
                    &server, "list_tools", kind, &detail,
                )));
            }
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
            Err(err) => {
                // F-19 Part 1: classify the failure + render as a
                // structured block. Replaces the pre-F-19 free-form
                // `"mcp_call_tool failed: {err}"` string that the
                // orchestrator LLM used to interpret (and fabricate
                // root causes for).
                let detail = err.to_string();
                let kind = classify_mcp_error(&detail);
                tracing::warn!(
                    target: "mcp",
                    server = %server,
                    tool = %tool,
                    kind = %kind.label(),
                    "[mcp_call_tool] failed: {detail}"
                );
                return Ok(ToolResult::error(render_mcp_tool_error(
                    &server, &tool, kind, &detail,
                )));
            }
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

    // ── F-19 Part 1: structured MCP tool errors ─────────────────────

    #[test]
    fn classify_mcp_error_recognises_auth_failed() {
        assert_eq!(
            super::classify_mcp_error("MCP unauthorized for `higgs` (HTTP 401 Bearer realm)"),
            super::McpToolErrorKind::AuthFailed
        );
        assert_eq!(
            super::classify_mcp_error("HTTP 403 Forbidden"),
            super::McpToolErrorKind::AuthFailed
        );
    }

    #[test]
    fn classify_mcp_error_recognises_endpoint_not_found() {
        assert_eq!(
            super::classify_mcp_error("MCP HTTP 404 — Not Found at POST /"),
            super::McpToolErrorKind::EndpointNotFound
        );
    }

    #[test]
    fn classify_mcp_error_recognises_server_error() {
        assert_eq!(
            super::classify_mcp_error("MCP HTTP 502 — Bad Gateway"),
            super::McpToolErrorKind::ServerError
        );
        assert_eq!(
            super::classify_mcp_error("HTTP 500 server error"),
            super::McpToolErrorKind::ServerError
        );
    }

    #[test]
    fn classify_mcp_error_recognises_unreachable() {
        assert_eq!(
            super::classify_mcp_error("error sending request for url (...): connect error"),
            super::McpToolErrorKind::EndpointUnreachable
        );
        assert_eq!(
            super::classify_mcp_error("dns error: name or service not known"),
            super::McpToolErrorKind::EndpointUnreachable
        );
    }

    #[test]
    fn classify_mcp_error_recognises_protocol_mismatch() {
        assert_eq!(
            super::classify_mcp_error("Failed to parse MCP JSON response: expected `{`"),
            super::McpToolErrorKind::ProtocolMismatch
        );
        assert_eq!(
            super::classify_mcp_error("parsing initialize result: missing field"),
            super::McpToolErrorKind::ProtocolMismatch
        );
    }

    #[test]
    fn classify_mcp_error_unknown_when_no_pattern_matches() {
        assert_eq!(
            super::classify_mcp_error("some completely novel failure mode"),
            super::McpToolErrorKind::Unknown
        );
    }

    #[test]
    fn classify_mcp_error_recognises_transient_session_lost() {
        // The Higgsfield repro: server returns 404 between initialize +
        // tools/list because the new session hasn't propagated yet.
        // Body says "Session not found" — must classify as TransientSessionLost,
        // NOT EndpointNotFound (which would misadvise the user to change
        // their endpoint path when the path is actually correct).
        assert_eq!(
            super::classify_mcp_error("MCP HTTP 404 — {\"error\": \"Session not found\"}"),
            super::McpToolErrorKind::TransientSessionLost
        );
        assert_eq!(
            super::classify_mcp_error("session expired with 404"),
            super::McpToolErrorKind::TransientSessionLost
        );
        assert_eq!(
            super::classify_mcp_error("Invalid session id"),
            super::McpToolErrorKind::TransientSessionLost
        );
    }

    #[test]
    fn render_mcp_tool_error_carries_stable_shape() {
        let rendered = super::render_mcp_tool_error(
            "Higgsfield",
            "generate_image",
            super::McpToolErrorKind::EndpointNotFound,
            "MCP HTTP 404 — POST /",
        );
        // The shape the orchestrator prompt teaches the LLM to surface
        // verbatim. Pin every labeled line.
        assert!(rendered.starts_with("⚠ MCP tool error\n"));
        assert!(rendered.contains("server: Higgsfield"));
        assert!(rendered.contains("tool: generate_image"));
        assert!(rendered.contains("kind: endpoint_not_found"));
        assert!(rendered.contains("detail: MCP HTTP 404 — POST /"));
        assert!(rendered.contains("suggestion: The endpoint path is likely wrong"));
        assert!(rendered.contains("Surface this block verbatim"));
    }

    #[test]
    fn render_mcp_tool_error_unknown_kind_explicitly_marks_it() {
        let rendered = super::render_mcp_tool_error(
            "Mystery",
            "do_something",
            super::McpToolErrorKind::Unknown,
            "weird internal failure",
        );
        assert!(rendered.contains("kind: unknown"));
        assert!(rendered.contains("Do NOT invent a root cause"));
    }
}
