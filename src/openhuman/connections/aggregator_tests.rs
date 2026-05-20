//! Aggregator tests — collector orchestration, ordering, error tolerance.

use super::*;
use crate::openhuman::config::Config;
use tempfile::TempDir;

fn config_with_workspace(dir: &TempDir) -> Config {
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    config
}

#[tokio::test]
async fn list_all_surfaces_builtin_and_mcp_on_fresh_workspace() {
    // Fresh workspace: composio/channels/webview/generic_http collectors all
    // contribute zero rows; builtin contributes 6 (the static catalog) and
    // mcp contributes the auto-registered `gitbooks` server.
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);
    let result = list_all(&config).await.unwrap();

    let kinds: Vec<ConnectionKind> = result
        .iter()
        .map(|v| ConnectionKind::from_ref(&v.r#ref))
        .collect();

    let builtin_count = kinds
        .iter()
        .filter(|k| **k == ConnectionKind::Builtin)
        .count();
    let mcp_count = kinds.iter().filter(|k| **k == ConnectionKind::Mcp).count();
    let other_count = kinds
        .iter()
        .filter(|k| **k != ConnectionKind::Builtin && **k != ConnectionKind::Mcp)
        .count();

    assert_eq!(
        builtin_count, 6,
        "all 6 built-in integrations should surface"
    );
    assert_eq!(mcp_count, 1, "legacy gitbooks MCP server should surface");
    assert_eq!(
        other_count, 0,
        "no other mechanism should populate on fresh workspace"
    );
}

#[tokio::test]
async fn list_all_is_deterministically_ordered() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    // Two calls on an empty workspace produce identical ordering across the
    // builtin + mcp rows surfaced by P0-6.
    let a = list_all(&config).await.unwrap();
    let b = list_all(&config).await.unwrap();
    assert_eq!(a, b, "aggregator ordering must be stable across calls");
}

#[tokio::test]
async fn generic_http_collector_handles_missing_table_idempotently() {
    // The `with_connection` opener applies migrations on first touch, so the
    // collector should never see a "table doesn't exist" error even on a
    // brand-new workspace.
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);
    let rows = collect_generic_http(&config).await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn builtin_collector_returns_six_integrations_without_session() {
    // No auth token in the fresh workspace → status should be NotConnected for
    // every integration, but the row count is fixed at 6.
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);
    let rows = collect_builtin(&config).await.unwrap();
    assert_eq!(rows.len(), 6);
    for row in &rows {
        assert_eq!(row.status, ConnectionStatus::NotConnected);
        assert_eq!(row.mechanism_label, "Built-in");
        assert!(matches!(row.r#ref, ConnectionRef::Builtin { .. }));
    }
}

#[tokio::test]
async fn mcp_collector_reports_legacy_gitbooks_server() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);
    let rows = collect_mcp(&config).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].display_name, "gitbooks");
    assert_eq!(rows[0].status, ConnectionStatus::Connected);
    assert!(matches!(rows[0].r#ref, ConnectionRef::Mcp { .. }));
}
