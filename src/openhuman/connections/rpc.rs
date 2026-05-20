//! JSON-RPC handlers for the Connections domain.
//!
//! Phase 0 / P0-2 shipped `connections_list`. P0-3 adds the Generic HTTP CRUD
//! + a connectivity-probe stub: `connections_generic_http_create/_update/
//! _delete` and `connections_test`. Real HTTP probe is deferred to P0-3a.

use crate::openhuman::config::Config;
use crate::openhuman::connections::types::{
    ConnectionKind, ConnectionsListRequest, ConnectionsListResponse, CreateGenericHttpRequest,
    GenericHttpConnection, TestProbeResult, UpdateGenericHttpRequest,
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

/// `openhuman.connections_test` — best-effort connectivity probe. **Phase 0 /
/// P0-3 ships a stub**: returns `ok: true` if the connection exists, with a
/// flag in `error` calling out the deferred real probe. P0-3a wires the
/// HEAD→OPTIONS→GET path against the OpenHuman `reqwest` client factory.
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

#[cfg(test)]
#[path = "rpc_tests.rs"]
mod tests;
