//! Aggregates per-mechanism connection state into a unified `Vec<ConnectionView>`.
//!
//! Six collectors, one per mechanism (composio, channel, webview, builtin, mcp,
//! generic_http). Each collector reads through the home domain's public read
//! API only. Failures degrade gracefully: a broken collector logs at `warn`
//! and contributes zero rows; the remaining mechanisms still populate.
//!
//! ## Current wiring status (P0-2)
//!
//! Only `collect_generic_http` is fully wired against this domain's own
//! `connections.db`. The other five collectors are stubs that return an empty
//! vector with a `TODO` log line; per-mechanism wiring is split into
//! follow-up tickets (P0-2a..P0-2e) to keep this PR small.
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
/// **Stubbed in P0-2** — follow-up ticket P0-2d wires this against the
/// `integrations` domain's enabled-flag pattern + scope config.
async fn collect_builtin(_config: &Config) -> Result<Vec<ConnectionView>> {
    tracing::debug!(target: "connections", "[connections] builtin collector — TODO P0-2d");
    Ok(Vec::new())
}

/// MCP servers + their exposed tools. **Stubbed in P0-2** — follow-up ticket
/// P0-2e wires this against `mcp_client`/`mcp_server` public registries.
async fn collect_mcp(_config: &Config) -> Result<Vec<ConnectionView>> {
    tracing::debug!(target: "connections", "[connections] mcp collector — TODO P0-2e");
    Ok(Vec::new())
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
