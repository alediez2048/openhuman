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
async fn list_all_surfaces_static_catalogs_on_fresh_workspace() {
    // Fresh workspace baseline after wiring channels + webview collectors:
    //   - 6 built-in integrations (twilio/apify/google_places/parallel/seltz/stock_prices)
    //   - 1 MCP server (auto-registered legacy `gitbooks`)
    //   - 4 chat channels (telegram/discord/web/imessage from all_channel_definitions)
    //   - 8 webview accounts (PROVIDERS list in webview_accounts/ops.rs)
    //   - 0 composio / generic_http (no session token, no DB rows)
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);
    let result = list_all(&config).await.unwrap();

    let count_of = |kind: ConnectionKind| -> usize {
        result
            .iter()
            .filter(|v| ConnectionKind::from_ref(&v.r#ref) == kind)
            .count()
    };

    assert_eq!(
        count_of(ConnectionKind::Builtin),
        6,
        "all 6 built-in integrations should surface"
    );
    assert_eq!(
        count_of(ConnectionKind::Mcp),
        1,
        "legacy gitbooks MCP server should surface"
    );
    assert_eq!(
        count_of(ConnectionKind::Channel),
        4,
        "all 4 channel definitions should surface (telegram/discord/web/imessage)"
    );
    assert_eq!(
        count_of(ConnectionKind::Webview),
        8,
        "all 8 webview providers should surface (gmail/whatsapp/telegram/slack/discord/linkedin/zoom/google_messages)"
    );
    assert_eq!(
        count_of(ConnectionKind::Composio),
        0,
        "composio collector should degrade to empty without a session token"
    );
    assert_eq!(
        count_of(ConnectionKind::GenericHttp),
        0,
        "no generic_http rows on a fresh workspace"
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
