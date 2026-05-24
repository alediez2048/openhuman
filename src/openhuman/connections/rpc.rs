//! JSON-RPC handlers for the Connections domain.
//!
//! Phase 0 / P0-2 shipped `connections_list`. P0-3 adds the Generic HTTP CRUD
//! + a connectivity-probe stub: `connections_generic_http_create/_update/
//! _delete` and `connections_test`. Real HTTP probe is deferred to P0-3a.

use crate::openhuman::config::{Config, McpServerConfig};
use crate::openhuman::connections::types::{
    ConnectionKind, ConnectionsListRequest, ConnectionsListResponse, CreateGenericHttpRequest,
    GenericHttpConnection, McpAddRequest, TestProbeResult, UpdateGenericHttpRequest,
};
use crate::openhuman::connections::{aggregator, ops};
use crate::rpc::RpcOutcome;

/// `openhuman.connections_list` — unified read across all 6 mechanisms.
pub async fn connections_list(
    config: &Config,
    req: ConnectionsListRequest,
) -> Result<RpcOutcome<ConnectionsListResponse>, String> {
    let mut connections = aggregator::list_all(config).await.map_err(|e| {
        tracing::error!(
            target: "connections",
            "[connections-rpc] connections_list aggregation failed: {e:#}"
        );
        format!("aggregator failed: {e}")
    })?;

    let pre_filter_count = connections.len();

    if let Some(kinds) = req.kind_filter.filter(|v| !v.is_empty()) {
        connections.retain(|c| kinds.contains(&ConnectionKind::from_ref(&c.r#ref)));
    }
    if let Some(query) = req.search {
        let needle = query.to_lowercase();
        if !needle.is_empty() {
            connections.retain(|c| c.display_name.to_lowercase().contains(&needle));
        }
    }

    let log = format!(
        "connections_list aggregated {pre_filter_count}, returning {}",
        connections.len()
    );
    Ok(RpcOutcome::single_log(
        ConnectionsListResponse {
            connections,
            generated_at: chrono::Utc::now(),
        },
        log,
    ))
}

/// `openhuman.connections_generic_http_create` — register a new Generic HTTP
/// connection. Encrypts the credential (if any) via `security/secrets` before
/// persisting.
pub async fn connections_generic_http_create(
    config: &Config,
    req: CreateGenericHttpRequest,
) -> Result<RpcOutcome<GenericHttpConnection>, String> {
    let conn = ops::create_generic_http(config, req)
        .await
        .map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        conn.clone(),
        format!("generic_http connection {} created", conn.id),
    ))
}

/// `openhuman.connections_generic_http_update` — partial update. `None`-valued
/// fields keep the existing value. `auth_credential = Some` rotates the secret.
pub async fn connections_generic_http_update(
    config: &Config,
    id: &str,
    req: UpdateGenericHttpRequest,
) -> Result<RpcOutcome<GenericHttpConnection>, String> {
    let conn = ops::update_generic_http(config, id, req)
        .await
        .map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        conn.clone(),
        format!("generic_http connection {} updated", conn.id),
    ))
}

/// `openhuman.connections_generic_http_delete` — remove the row + publish
/// `ConnectionRemoved`. Idempotent: returns `false` if the id was unknown.
pub async fn connections_generic_http_delete(
    config: &Config,
    id: &str,
) -> Result<RpcOutcome<bool>, String> {
    let removed = ops::delete_generic_http(config, id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        removed,
        format!("generic_http connection {id} delete: removed={}", removed),
    ))
}

/// `openhuman.connections_test` — real HTTP connectivity probe (P0-3a).
/// HEAD → OPTIONS → GET(Range:0-0) fallback chain via the OpenHuman
/// reqwest factory. Writes the outcome into the in-memory verification
/// cache so the next `connections_list` reflects Live/Failed.
pub async fn connections_test(
    config: &Config,
    id: &str,
) -> Result<RpcOutcome<TestProbeResult>, String> {
    let result = ops::test_generic_http(config, id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        result.clone(),
        format!("connections_test id={id} ok={}", result.ok),
    ))
}

/// `openhuman.connections_generic_http_get` — fetch the full saved row
/// for a Generic HTTP connection by id. Used by the manage modal so the
/// form is populated with real persisted values (base_url, auth_kind,
/// default_headers) rather than a frontend-constructed stub.
pub async fn connections_generic_http_get(
    config: &Config,
    id: &str,
) -> Result<RpcOutcome<Option<GenericHttpConnection>>, String> {
    let row = ops::get_generic_http(config, id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        row.clone(),
        format!(
            "connections_generic_http_get id={id} found={}",
            row.is_some()
        ),
    ))
}

/// `openhuman.connections_mcp_test` — real MCP `initialize` probe.
/// Calls the server's JSON-RPC `initialize` and records the outcome in
/// the verification cache. 15s wall-clock timeout.
pub async fn connections_mcp_test(
    config: &Config,
    server_id: &str,
) -> Result<RpcOutcome<TestProbeResult>, String> {
    let result = ops::test_mcp_server(config, server_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        result.clone(),
        format!(
            "connections_mcp_test server_id={server_id} ok={}",
            result.ok
        ),
    ))
}

/// `openhuman.connections_mcp_add` — register a new MCP server in
/// `config.mcp_client.servers` and persist the TOML. The aggregator's
/// `collect_mcp` builds a fresh registry on every call, so the new
/// server surfaces on the next `connections_list` without a core restart.
pub async fn connections_mcp_add(
    config: &Config,
    req: McpAddRequest,
) -> Result<RpcOutcome<McpServerConfig>, String> {
    let server = ops::add_mcp_server(config, req)
        .await
        .map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        server.clone(),
        format!("mcp server {} registered", server.name),
    ))
}

/// `openhuman.connections_mcp_remove` — remove an MCP server by name.
/// Idempotent: returns `removed=false` when the name was unknown.
pub async fn connections_mcp_remove(
    config: &Config,
    name: &str,
) -> Result<RpcOutcome<bool>, String> {
    let removed = ops::remove_mcp_server(config, name)
        .await
        .map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        removed,
        format!("mcp server {name} remove: removed={removed}"),
    ))
}

/// `openhuman.connections_mcp_orphans_list` — F-18 Part 3: surface MCP
/// servers registered under a previous-session user dir. Used by the
/// `/connections` UI to render the "restore previous-session
/// credentials" banner. Tokens are redacted; the bearer secret never
/// crosses this RPC boundary.
pub async fn connections_mcp_orphans_list(
    config: &Config,
) -> Result<RpcOutcome<ops::McpOrphanListing>, String> {
    let listing = ops::list_mcp_orphans(config)
        .await
        .map_err(|e| e.to_string())?;
    let msg = format!(
        "mcp-orphan scan: {} orphan(s) across {} non-active user dir(s){}",
        listing.orphans.len(),
        listing.user_dirs_scanned,
        if listing.capped { " (capped)" } else { "" }
    );
    Ok(RpcOutcome::single_log(listing, msg))
}

/// `openhuman.connections_mcp_orphans_migrate` — F-18 Part 3: copy one
/// orphan MCP server into the active user's config. Reads the source
/// user's full server entry (including the bearer token) server-side
/// and re-uses the regular `add_mcp_server` ops so the dedup check,
/// stale-handle guard, and `ConnectionAdded` event publish all fire.
/// Does NOT delete from the source.
pub async fn connections_mcp_orphans_migrate(
    config: &Config,
    source_user_id: &str,
    server_name: &str,
) -> Result<RpcOutcome<crate::openhuman::config::McpServerConfig>, String> {
    let server = ops::migrate_mcp_orphan(config, source_user_id, server_name)
        .await
        .map_err(|e| e.to_string())?;
    let msg = format!(
        "mcp orphan migrated: {} (from user {source_user_id}) → active user",
        server.name
    );
    Ok(RpcOutcome::single_log(server, msg))
}

#[cfg(test)]
#[path = "rpc_tests.rs"]
mod tests;
