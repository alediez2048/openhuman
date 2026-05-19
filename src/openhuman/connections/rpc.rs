//! JSON-RPC handlers for the Connections domain.
//!
//! Phase 0 / P0-2 ships the read-side: `connections_list`. P0-3 adds the
//! Generic HTTP CRUD + connectivity probe (`connections_generic_http_*`,
//! `connections_test`).

use crate::openhuman::config::Config;
use crate::openhuman::connections::aggregator;
use crate::openhuman::connections::types::{
    ConnectionKind, ConnectionsListRequest, ConnectionsListResponse,
};
use crate::rpc::RpcOutcome;

/// Implements the `openhuman.connections_list` RPC.
///
/// Calls [`aggregator::list_all`], then applies the optional `kind_filter` and
/// case-insensitive `search` post-aggregation. N is small (typically `< 200`)
/// so filtering in process is fine.
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

#[cfg(test)]
#[path = "rpc_tests.rs"]
mod tests;
