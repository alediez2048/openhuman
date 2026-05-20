//! Aggregates per-mechanism connection state into a unified `Vec<ConnectionView>`.
//!
//! Six collectors, one per mechanism (composio, channel, webview, builtin, mcp,
//! generic_http). Each collector reads through the home domain's public read
//! API only. Failures degrade gracefully: a broken collector logs at `warn`
//! and contributes zero rows; the remaining mechanisms still populate.
//!
//! ## Current wiring status
//!
//! Wired:
//! - `collect_generic_http` (P0-2) — reads this domain's own `connections.db`.
//! - `collect_builtin` (P0-6) — enumerates the 6 backend-proxied agent
//!   integrations (twilio/apify/google_places/parallel/seltz/stock_prices)
//!   and derives status from the presence of an `IntegrationClient`.
//! - `collect_mcp` (P0-6) — reads `McpServerRegistry::from_config(config)`.
//!
//! Still stubbed (return `Vec::new()`):
//! - `collect_composio` (P0-2a), `collect_channels` (P0-2b),
//!   `collect_webview` (P0-2c).
//!
//! See `Automations/systemsdesign.md §2.1`, `ADR-003`, `ADR-006`.

use crate::openhuman::config::Config;
use crate::openhuman::connections::store;
use crate::openhuman::connections::types::{
    ConnectionKind, ConnectionRef, ConnectionStatus, ConnectionView,
};
use anyhow::Result;

/// Collects every connection across all 6 mechanisms in parallel.
///
/// Returns a deterministically-ordered `Vec<ConnectionView>` (stable-sorted by
/// `kind` then `display_name`). Individual collector failures are logged and
/// suppressed.
pub async fn list_all(config: &Config) -> Result<Vec<ConnectionView>> {
    let (composio, channels, webview, builtin, mcp, generic_http) = tokio::join!(
        collect_composio(config),
        collect_channels(config),
        collect_webview(config),
        collect_builtin(config),
        collect_mcp(config),
        collect_generic_http(config),
    );

    let mut all = Vec::new();
    for (label, result) in [
        ("composio", composio),
        ("channels", channels),
        ("webview", webview),
        ("builtin", builtin),
        ("mcp", mcp),
        ("generic_http", generic_http),
    ] {
        match result {
            Ok(mut rows) => {
                tracing::debug!(
                    target: "connections",
                    "[connections] {label} collector returned N={}",
                    rows.len()
                );
                all.append(&mut rows);
            }
            Err(e) => {
                tracing::warn!(
                    target: "connections",
                    "[connections] {label} collector failed: {e:#}"
                );
            }
        }
    }

    all.sort_by(|a, b| {
        let kind_a = ConnectionKind::from_ref(&a.r#ref);
        let kind_b = ConnectionKind::from_ref(&b.r#ref);
        (kind_a.as_str(), &a.display_name).cmp(&(kind_b.as_str(), &b.display_name))
    });

    Ok(all)
}

/// Composio toolkits + connected accounts. **Stubbed in P0-2** — follow-up
/// ticket P0-2a wires this against `composio::ops::list_connected_toolkits`.
async fn collect_composio(_config: &Config) -> Result<Vec<ConnectionView>> {
    tracing::debug!(target: "connections", "[connections] composio collector — TODO P0-2a");
    Ok(Vec::new())
}

/// Native chat-channel providers (Slack/Discord/Telegram/...). **Stubbed in
/// P0-2** — follow-up ticket P0-2b wires this against `channels` public APIs.
async fn collect_channels(_config: &Config) -> Result<Vec<ConnectionView>> {
    tracing::debug!(target: "connections", "[connections] channels collector — TODO P0-2b");
    Ok(Vec::new())
}

/// CEF-hosted webview accounts (LinkedIn/Twitter/WhatsApp/...). **Stubbed in
/// P0-2** — follow-up ticket P0-2c wires this against the Tauri-side webview
/// account registry via a new read RPC.
async fn collect_webview(_config: &Config) -> Result<Vec<ConnectionView>> {
    tracing::debug!(target: "connections", "[connections] webview collector — TODO P0-2c");
    Ok(Vec::new())
}

/// OpenHuman-backend-proxied built-in integrations (Twilio/Apify/...).
///
/// These are not local-config-toggled — they are agent tools that proxy
/// through the OpenHuman backend and are gated by:
/// 1. Presence of a session JWT (resolved by `integrations::build_client`).
/// 2. Per-account availability (delivered by the backend pricing endpoint).
///
/// Phase 0 surfaces them read-only. Status is derived from (1): when
/// `build_client(&config)` returns `Some`, every integration is reported as
/// `Connected`; otherwise `NotConnected` with a generic
/// "Sign in to enable" affordance owned by the frontend section.
///
/// Toggle / credential rotation lands as a follow-up (`P0-6a`) once the
/// backend exposes a per-account integration-enabled surface.
async fn collect_builtin(config: &Config) -> Result<Vec<ConnectionView>> {
    /// `(integration_id, display_label)` for every backend-proxied integration.
    /// Mirrors the public surface in `openhuman/integrations/mod.rs`.
    const BUILTINS: &[(&str, &str)] = &[
        ("twilio", "Twilio"),
        ("apify", "Apify"),
        ("google_places", "Google Places"),
        ("parallel", "Parallel"),
        ("seltz", "Seltz"),
        ("stock_prices", "Stock Prices"),
    ];

    // `build_client` does file I/O to read the session JWT — wrap in
    // spawn_blocking so we never block the async runtime.
    let config_clone = config.clone();
    let has_session = tokio::task::spawn_blocking(move || {
        crate::openhuman::integrations::build_client(&config_clone).is_some()
    })
    .await
    .map_err(|e| anyhow::anyhow!("builtin collector task panicked: {e}"))?;

    let status = if has_session {
        ConnectionStatus::Connected
    } else {
        ConnectionStatus::NotConnected
    };

    tracing::debug!(
        target: "connections",
        "[connections] builtin collector — has_session={has_session} count={}",
        BUILTINS.len()
    );

    Ok(BUILTINS
        .iter()
        .map(|(id, label)| ConnectionView {
            r#ref: ConnectionRef::Builtin {
                integration: (*id).to_string(),
            },
            display_name: (*label).to_string(),
            status: status.clone(),
            last_used_at: None,
            mechanism_label: "Built-in".to_string(),
        })
        .collect())
}

/// MCP servers configured under `config.mcp_client.servers` plus the legacy
/// `gitbooks` server when `config.gitbooks.enabled`.
///
/// Read-only in Phase 0. Restart / enable / disable lands as `P0-6b`; today
/// the registry has no in-process "restart" verb (HTTP clients are lazy,
/// stdio clients are per-call), so this collector only surfaces presence.
async fn collect_mcp(config: &Config) -> Result<Vec<ConnectionView>> {
    let registry = crate::openhuman::mcp_client::McpServerRegistry::from_config(config);

    let rows: Vec<ConnectionView> = registry
        .list()
        .into_iter()
        .map(|def| ConnectionView {
            r#ref: ConnectionRef::Mcp {
                server_id: def.name.clone(),
                tool_name: None,
            },
            display_name: def.name.clone(),
            status: ConnectionStatus::Connected,
            last_used_at: None,
            mechanism_label: "MCP".to_string(),
        })
        .collect();

    tracing::debug!(
        target: "connections",
        "[connections] mcp collector — count={}",
        rows.len()
    );

    Ok(rows)
}

/// Generic HTTP connection rows from this domain's own `connections.db`.
/// Fully wired in P0-2 — this is the one collector that doesn't depend on
/// another mechanism's API.
async fn collect_generic_http(config: &Config) -> Result<Vec<ConnectionView>> {
    let config = config.clone();
    let rows = tokio::task::spawn_blocking(move || store::list_generic_http(&config))
        .await
        .map_err(|e| anyhow::anyhow!("generic_http collector task panicked: {e}"))??;

    Ok(rows
        .into_iter()
        .map(|row| ConnectionView {
            r#ref: ConnectionRef::GenericHttp {
                connection_id: row.id.clone(),
            },
            display_name: row.name,
            status: ConnectionStatus::Connected,
            last_used_at: Some(row.updated_at),
            mechanism_label: "Generic HTTP".to_string(),
        })
        .collect())
}

#[cfg(test)]
#[path = "aggregator_tests.rs"]
mod tests;
