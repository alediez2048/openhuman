//! Tests for `connections_list` RPC: filter/search post-aggregation.

use super::*;
use crate::openhuman::config::Config;
use crate::openhuman::connections::types::{ConnectionRef, ConnectionStatus, ConnectionView};
use tempfile::TempDir;

fn config_with_workspace(dir: &TempDir) -> Config {
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    config
}

/// After P0-6, a fresh workspace surfaces:
/// - 6 built-in integration rows (twilio/apify/google_places/parallel/seltz/stock_prices)
/// - 1 MCP server row (the legacy `gitbooks` auto-registration)
/// — and nothing else (composio/channels/webview/generic_http remain empty).
const FRESH_BASELINE_ROWS: usize = 7;

#[tokio::test]
async fn connections_list_fresh_workspace_returns_builtin_and_mcp_baseline() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let outcome = connections_list(&config, ConnectionsListRequest::default())
        .await
        .unwrap();
    assert_eq!(outcome.value.connections.len(), FRESH_BASELINE_ROWS);
    assert!(
        !outcome.logs.is_empty(),
        "should log the aggregation summary"
    );
}

#[tokio::test]
async fn connections_list_search_does_not_panic_on_empty_input() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let req = ConnectionsListRequest {
        kind_filter: None,
        search: Some("".to_string()),
    };
    let outcome = connections_list(&config, req).await.unwrap();
    // Empty search string short-circuits to a no-op filter (still returns the
    // P0-6 fresh-workspace baseline).
    assert_eq!(outcome.value.connections.len(), FRESH_BASELINE_ROWS);
}

#[tokio::test]
async fn connections_list_kind_filter_with_empty_vec_is_no_op() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let req = ConnectionsListRequest {
        kind_filter: Some(vec![]),
        search: None,
    };
    let outcome = connections_list(&config, req).await.unwrap();
    // empty kind_filter is treated as "no filter" — no retain pass invoked.
    assert_eq!(outcome.value.connections.len(), FRESH_BASELINE_ROWS);
}

#[tokio::test]
async fn connections_list_kind_filter_isolates_generic_http_from_baseline() {
    // With the P0-6 baseline of 7 rows, a `GenericHttp`-only filter on a fresh
    // workspace should produce zero rows (the baseline has no generic_http).
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let req = ConnectionsListRequest {
        kind_filter: Some(vec![ConnectionKind::GenericHttp]),
        search: None,
    };
    let outcome = connections_list(&config, req).await.unwrap();
    assert!(outcome.value.connections.is_empty());
}

#[test]
fn kind_filter_logic_excludes_non_matching_views() {
    // Pure-logic test (no I/O): manual ConnectionView fixtures, apply the same
    // retain logic the RPC uses.
    let mut views = vec![
        ConnectionView {
            r#ref: ConnectionRef::Composio {
                toolkit_id: "gmail".into(),
                account_id: None,
            },
            display_name: "Gmail".into(),
            status: ConnectionStatus::Connected,
            last_used_at: None,
            mechanism_label: "Composio".to_string(),
        },
        ConnectionView {
            r#ref: ConnectionRef::Channel {
                provider: "telegram".into(),
                channel_id: "@jad".into(),
            },
            display_name: "Telegram".into(),
            status: ConnectionStatus::Connected,
            last_used_at: None,
            mechanism_label: "Channel".to_string(),
        },
    ];

    let allow = vec![ConnectionKind::Composio];
    views.retain(|c| allow.contains(&ConnectionKind::from_ref(&c.r#ref)));

    assert_eq!(views.len(), 1);
    assert_eq!(views[0].display_name, "Gmail");
}

#[test]
fn search_substring_matches_display_name_case_insensitively() {
    let mut views = vec![
        ConnectionView {
            r#ref: ConnectionRef::Composio {
                toolkit_id: "gmail".into(),
                account_id: None,
            },
            display_name: "Gmail Personal".into(),
            status: ConnectionStatus::Connected,
            last_used_at: None,
            mechanism_label: "Composio".to_string(),
        },
        ConnectionView {
            r#ref: ConnectionRef::Composio {
                toolkit_id: "linear".into(),
                account_id: None,
            },
            display_name: "Linear".into(),
            status: ConnectionStatus::Connected,
            last_used_at: None,
            mechanism_label: "Composio".to_string(),
        },
    ];

    let needle = "GMAIL".to_lowercase();
    views.retain(|c| c.display_name.to_lowercase().contains(&needle));

    assert_eq!(views.len(), 1);
    assert_eq!(views[0].display_name, "Gmail Personal");
}
