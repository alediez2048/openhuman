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

/// Reference to a secret encrypted via `src/openhuman/security/secrets`.
///
/// Carries the `enc2:<hex>` ChaCha20-Poly1305 ciphertext produced by
/// `SecretStore::encrypt`. The cleartext credential is never persisted; it
/// only exists in memory during `create_generic_http` / `update_generic_http`
/// and is decrypted at run time when `resolve_auth_header` needs it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretRef {
    /// `enc2:<hex>` blob produced by `SecretStore::encrypt`.
    pub ciphertext: String,
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
    ///
    /// Stored as `String` (not `&'static str`) so the type is `Deserialize`-
    /// able for the `connections_list` RPC response shape.
    pub mechanism_label: String,
}

/// Compact discriminator used for filtering responses from `connections_list`.
///
/// Mirrors the structural variants of [`ConnectionRef`] but flat — useful for
/// UI filter chips and `kind_filter` request params.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    Composio,
    Channel,
    Webview,
    Builtin,
    Mcp,
    GenericHttp,
}

impl ConnectionKind {
    /// Returns the matching kind for a given [`ConnectionRef`].
    pub fn from_ref(r#ref: &ConnectionRef) -> Self {
        match r#ref {
            ConnectionRef::Composio { .. } => Self::Composio,
            ConnectionRef::Channel { .. } => Self::Channel,
            ConnectionRef::Webview { .. } => Self::Webview,
            ConnectionRef::Builtin { .. } => Self::Builtin,
            ConnectionRef::Mcp { .. } => Self::Mcp,
            ConnectionRef::GenericHttp { .. } => Self::GenericHttp,
        }
    }

    /// Stable lowercase slug used in serialized payloads and log lines.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Composio => "composio",
            Self::Channel => "channel",
            Self::Webview => "webview",
            Self::Builtin => "builtin",
            Self::Mcp => "mcp",
            Self::GenericHttp => "generic_http",
        }
    }
}

/// Request payload for the `connections_list` RPC.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionsListRequest {
    /// Optional kind allowlist. When `None` or empty, all kinds are returned.
    #[serde(default)]
    pub kind_filter: Option<Vec<ConnectionKind>>,
    /// Optional case-insensitive substring match against `display_name`.
    #[serde(default)]
    pub search: Option<String>,
}

/// Response payload for the `connections_list` RPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionsListResponse {
    /// Unified, filter-applied list of connections.
    pub connections: Vec<ConnectionView>,
    /// Wall-clock timestamp the aggregation completed at.
    pub generated_at: DateTime<Utc>,
}

// ── Generic HTTP CRUD payloads (P0-3) ────────────────────────────────────

/// Cleartext credential about to be moved into `security/secrets`.
///
/// **In-memory only.** This type is the input to `create_generic_http` /
/// `update_generic_http` and never reaches the database or any RPC response.
/// After `SecretStore::encrypt` runs, the resulting `enc2:<hex>` blob is
/// wrapped in a `SecretRef` and persisted; the original `NewCredential` is
/// dropped.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewCredential {
    /// Cleartext credential (bearer token, basic auth string, API key, etc.).
    pub secret: String,
}

/// Request payload for `connections_generic_http_create`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateGenericHttpRequest {
    pub name: String,
    pub base_url: String,
    pub auth_kind: AuthKind,
    /// Cleartext credential. `None` when `auth_kind = None`.
    #[serde(default)]
    pub auth_credential: Option<NewCredential>,
    #[serde(default)]
    pub default_headers: Vec<(String, String)>,
}

/// Request payload for `connections_generic_http_update`. Every field is
/// optional — `None` means "do not change".
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateGenericHttpRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth_kind: Option<AuthKind>,
    /// New cleartext credential. `Some` rotates the secret; `None` leaves the
    /// existing `secret_ref` untouched.
    #[serde(default)]
    pub auth_credential: Option<NewCredential>,
    #[serde(default)]
    pub default_headers: Option<Vec<(String, String)>>,
}

/// Result of a `connections_test` probe. Best-effort — failures return a
/// structured payload rather than an `Err`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TestProbeResult {
    /// `true` when the probe got a 2xx/3xx response.
    pub ok: bool,
    /// HTTP status code if a response was received.
    #[serde(default)]
    pub status: Option<u16>,
    /// Error message if the probe failed before getting a response (timeout,
    /// connect error, DNS failure, etc.).
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
