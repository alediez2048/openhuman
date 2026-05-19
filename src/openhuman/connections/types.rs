//! Types for the Connections domain (Phase 0 — Workflows & Automations).
//!
//! `ConnectionRef` is the discriminated union the workflows domain (and any other
//! caller) uses to reference a connected service. The aggregator in `aggregator.rs`
//! returns `Vec<ConnectionView>` rows assembled from the existing mechanism stores
//! (composio, channels, webview accounts, integrations, MCP) plus the
//! `GenericHttpConnection` rows owned by this domain.
//!
//! See `Automations/systemsdesign.md §2.1` and `Automations/ADRs/ADR-003`.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Stable id for a Generic HTTP connection row (UUIDv7 string).
pub type GenericHttpConnectionId = String;

/// Reference to a secret stored in `src/openhuman/security/secrets`.
///
/// Phase 0 stores the secret's stable name. The actual value is resolved at
/// run time and never serialized into workflow definitions or run records.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretRef {
    /// Logical name of the secret in the workspace secret store.
    pub name: String,
}

/// Identifier for a connected service across all six mechanisms.
///
/// Phase 0 owns the `GenericHttp` variant. The other five variants are
/// "soft references" into existing domains — the aggregator reads through
/// composio/channels/webview/integrations/MCP to populate them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConnectionRef {
    /// Composio toolkit + optional account id.
    Composio {
        toolkit_id: String,
        #[serde(default)]
        account_id: Option<String>,
    },
    /// Native chat-channel provider (slack/discord/telegram/...).
    Channel {
        provider: String,
        channel_id: String,
    },
    /// CEF-hosted webview account (linkedin/twitter/whatsapp/...).
    Webview {
        provider: String,
        account_id: String,
    },
    /// OpenHuman-backend-proxied built-in integration (twilio/apify/...).
    Builtin { integration: String },
    /// MCP server + optional specific tool name.
    Mcp {
        server_id: String,
        #[serde(default)]
        tool_name: Option<String>,
    },
    /// Generic HTTP connection owned by the `connections` domain.
    GenericHttp {
        connection_id: GenericHttpConnectionId,
    },
}

/// How a Generic HTTP connection authenticates outbound requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthKind {
    /// No authentication header is added.
    #[default]
    None,
    /// `Authorization: Bearer <secret>` header.
    Bearer,
    /// `Authorization: Basic <secret>` header.
    Basic,
    /// Custom header carrying the secret (e.g. `X-API-Key`).
    ApiKeyHeader { name: String },
    /// Auth credential passed as a query string parameter.
    QueryParam { name: String },
}

/// User-defined REST endpoint that workflows can target via the `http_request`
/// node (introduced in Phase 2). Phase 0 stores rows; Phase 2 consumes them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenericHttpConnection {
    pub id: GenericHttpConnectionId,
    pub name: String,
    pub base_url: String,
    pub auth_kind: AuthKind,
    /// Reference into `security/secrets`. `None` when `auth_kind` is `None`.
    #[serde(default)]
    pub secret_ref: Option<SecretRef>,
    /// Default headers applied to every request through this connection.
    #[serde(default)]
    pub default_headers: Vec<(String, String)>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Liveness state of a connection from the user's perspective.
///
/// Populated by `aggregator.rs` based on per-mechanism reads. `Connected` for
/// fully usable connections; everything else surfaces in the UI as a warning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectionStatus {
    Connected,
    NotConnected,
    Disabled,
    Error { reason: String },
}

/// Unified read-model surfaced by `connections_list`. Composes a `ConnectionRef`
/// with display metadata so the frontend can render a card without doing six
/// per-mechanism lookups.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionView {
    #[serde(rename = "ref")]
    pub r#ref: ConnectionRef,
    pub display_name: String,
    pub status: ConnectionStatus,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    /// Human-readable mechanism label: "Composio", "Channel", "Webview",
    /// "Built-in", "MCP", "Generic HTTP".
    pub mechanism_label: &'static str,
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
