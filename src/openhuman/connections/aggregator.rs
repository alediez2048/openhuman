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
//! - `collect_composio` (P0-2a) — calls `composio::ops::composio_list_connections`
//!   under a `COMPOSIO_COLLECTOR_TIMEOUT` so a slow / failing Composio call
//!   degrades the section to empty rather than blocking the whole hub.
//! - `collect_channels` (P0-2b) — enumerates every channel via
//!   `channels::controllers::all_channel_definitions()` and marks each row
//!   `Connected` when its slug is in `connected_channel_slugs(config)`.
//! - `collect_webview` (P0-2c) — calls `webview_accounts::detect_webview_logins`
//!   and emits one row per webview provider (`whatsapp`, `linkedin`,
//!   `slack`, `telegram`, …) with status keyed off the cookie probe.
//!
//! See `Automations/systemsdesign.md §2.1`, `ADR-003`, `ADR-006`.

use crate::openhuman::config::Config;
use crate::openhuman::connections::store;
use crate::openhuman::connections::types::{
    ConnectionKind, ConnectionRef, ConnectionStatus, ConnectionView,
};
use anyhow::Result;
use std::time::Duration;

/// Hard ceiling on the Composio HTTP call inside the aggregator. Composio
/// network round-trips can stretch under load; without a bound, every Hub
/// page load would wait on the worst tail. 3s is generous for a healthy
/// backend and short enough that a regression doesn't make the Hub feel
/// broken.
const COMPOSIO_COLLECTOR_TIMEOUT: Duration = Duration::from_secs(3);

/// Uppercase the first ASCII character of `s` for display.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

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

/// Composio connected accounts (`composio::ops::composio_list_connections`).
///
/// One `ConnectionView` per active Composio connection. `display_name`
/// prefers the account email when present, otherwise falls back to the
/// toolkit slug. Status mirrors the Composio backend status field:
/// `ACTIVE` / `CONNECTED` → `Connected`, everything else → `NotConnected`
/// (the UI surfaces error states the same way Composio's own dashboards do).
///
/// Failures (no session token, network errors, timeouts) degrade to an
/// empty result + a `warn!` log; the rest of the aggregator still
/// populates. See `COMPOSIO_COLLECTOR_TIMEOUT`.
async fn collect_composio(config: &Config) -> Result<Vec<ConnectionView>> {
    let call = crate::openhuman::composio::ops::composio_list_connections(config);
    let outcome = match tokio::time::timeout(COMPOSIO_COLLECTOR_TIMEOUT, call).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => {
            tracing::warn!(
                target: "connections",
                "[connections] composio collector skipped: {e}"
            );
            return Ok(Vec::new());
        }
        Err(_elapsed) => {
            tracing::warn!(
                target: "connections",
                "[connections] composio collector timed out after {}s",
                COMPOSIO_COLLECTOR_TIMEOUT.as_secs()
            );
            return Ok(Vec::new());
        }
    };

    let rows: Vec<ConnectionView> = outcome
        .value
        .connections
        .into_iter()
        .map(|c| {
            // Mirror the frontend's `deriveComposioState` mapping so the
            // section can render "Connected" / "Error" / "Not connected"
            // exactly like the legacy Skills grid did. Pending / initiated
            // states fall under `NotConnected` here because the user has
            // not yet completed OAuth.
            let status = match c.status.to_uppercase().as_str() {
                "ACTIVE" | "CONNECTED" => ConnectionStatus::Connected,
                "FAILED" | "ERROR" => ConnectionStatus::Error {
                    reason: format!("Composio status: {}", c.status),
                },
                "EXPIRED" => ConnectionStatus::Error {
                    reason: "Composio credential expired — reconnect required".to_string(),
                },
                _ => ConnectionStatus::NotConnected,
            };
            // Capitalize the toolkit slug for the aggregator's fallback
            // display name ("gmail" → "Gmail"). The Composio section in
            // the UI overrides this with `composioToolkitMeta(slug).name`
            // for a canonical "Google Calendar" label.
            let display_name = capitalize(&c.toolkit);
            ConnectionView {
                r#ref: ConnectionRef::Composio {
                    toolkit_id: c.toolkit.clone(),
                    account_id: Some(c.id.clone()),
                },
                display_name,
                status,
                last_used_at: None,
                mechanism_label: "Composio".to_string(),
            }
        })
        .collect();

    tracing::debug!(
        target: "connections",
        "[connections] composio collector — count={}",
        rows.len()
    );

    Ok(rows)
}

/// Native chat-channel providers (Slack/Discord/Telegram/...).
///
/// Surfaces every channel in the canonical catalog
/// (`channels::controllers::all_channel_definitions`) with status keyed off
/// the merged "is this slug connected" check in
/// `channels::controllers::connected_channel_slugs` — the same authoritative
/// source the chat runtime uses. Channels with no auth yet appear as
/// `NotConnected` so the UI can offer a setup CTA.
///
/// Failures inside the credentials read degrade to an empty result with a
/// `warn!` log so the rest of the aggregator still populates.
async fn collect_channels(config: &Config) -> Result<Vec<ConnectionView>> {
    let defs = crate::openhuman::channels::controllers::all_channel_definitions();
    let connected: std::collections::HashSet<String> =
        match crate::openhuman::channels::controllers::connected_channel_slugs(config).await {
            Ok(slugs) => slugs.into_iter().collect(),
            Err(e) => {
                tracing::warn!(
                    target: "connections",
                    "[connections] channels collector — connected_channel_slugs failed: {e}"
                );
                std::collections::HashSet::new()
            }
        };

    let rows: Vec<ConnectionView> = defs
        .into_iter()
        .map(|def| {
            let slug = def.id.to_string();
            let is_connected = connected.contains(&slug);
            ConnectionView {
                r#ref: ConnectionRef::Channel {
                    provider: slug.clone(),
                    channel_id: slug.clone(),
                },
                display_name: def.display_name.to_string(),
                status: if is_connected {
                    ConnectionStatus::Connected
                } else {
                    ConnectionStatus::NotConnected
                },
                last_used_at: None,
                mechanism_label: "Channel".to_string(),
            }
        })
        .collect();

    tracing::debug!(
        target: "connections",
        "[connections] channels collector — count={} connected={}",
        rows.len(),
        connected.len()
    );

    Ok(rows)
}

/// CEF-hosted webview accounts (Gmail / WhatsApp / Telegram / Slack / Discord
/// / LinkedIn / Zoom / Google Messages).
///
/// Reads the cookie-store login heuristic exposed by
/// `webview_accounts::detect_webview_logins`. Every supported provider
/// surfaces as a row so the UI can show "Add account" CTAs for the ones the
/// user hasn't logged into yet. `detect_webview_logins` never errors — at
/// worst it reports all providers as logged-out.
async fn collect_webview(_config: &Config) -> Result<Vec<ConnectionView>> {
    let logins = crate::openhuman::webview_accounts::detect_webview_logins();
    let Some(map) = logins.as_object() else {
        tracing::warn!(
            target: "connections",
            "[connections] webview collector — detect_webview_logins did not return an object"
        );
        return Ok(Vec::new());
    };

    let rows: Vec<ConnectionView> = map
        .iter()
        .map(|(slug, value)| {
            let is_connected = value.as_bool().unwrap_or(false);
            ConnectionView {
                r#ref: ConnectionRef::Webview {
                    provider: slug.clone(),
                    account_id: slug.clone(),
                },
                display_name: capitalize_webview_label(slug),
                status: if is_connected {
                    ConnectionStatus::Connected
                } else {
                    ConnectionStatus::NotConnected
                },
                last_used_at: None,
                mechanism_label: "Browser Account".to_string(),
            }
        })
        .collect();

    tracing::debug!(
        target: "connections",
        "[connections] webview collector — count={}",
        rows.len()
    );

    Ok(rows)
}

/// Friendly label for a webview provider slug. Falls back to the
/// uppercased-first-letter slug when the provider isn't in the curated map.
fn capitalize_webview_label(slug: &str) -> String {
    match slug {
        "whatsapp" => "WhatsApp".to_string(),
        "telegram" => "Telegram".to_string(),
        "slack" => "Slack".to_string(),
        "discord" => "Discord".to_string(),
        "linkedin" => "LinkedIn".to_string(),
        "twitter" => "X (Twitter)".to_string(),
        "instagram" => "Instagram".to_string(),
        "messenger" => "Messenger".to_string(),
        _ => capitalize(slug),
    }
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
