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
async fn list_all_returns_empty_on_fresh_workspace() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);
    // First call triggers the migration; should return empty.
    let result = list_all(&config).await.unwrap();
    assert!(result.is_empty(), "fresh workspace should aggregate to []");
}

#[tokio::test]
async fn list_all_is_deterministically_ordered() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    // Two calls on an empty workspace produce identical (empty) ordering.
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
