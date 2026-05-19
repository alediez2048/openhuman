//! Serde round-trip tests for connection types.
use super::*;
use chrono::TimeZone;
use serde_json;

#[test]
fn connection_ref_composio_round_trip() {
    let original = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: Some("jad@example.com".into()),
    };
    let json = serde_json::to_string(&original).unwrap();
    let parsed: ConnectionRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
    assert!(json.contains(r#""type":"composio""#));
}

#[test]
fn connection_ref_composio_without_account() {
    let original = ConnectionRef::Composio {
        toolkit_id: "linear".into(),
        account_id: None,
    };
    let json = serde_json::to_string(&original).unwrap();
    let parsed: ConnectionRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn connection_ref_channel_round_trip() {
    let original = ConnectionRef::Channel {
        provider: "telegram".into(),
        channel_id: "channel-123".into(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let parsed: ConnectionRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
    assert!(json.contains(r#""type":"channel""#));
}

#[test]
fn connection_ref_webview_round_trip() {
    let original = ConnectionRef::Webview {
        provider: "linkedin".into(),
        account_id: "acct_42".into(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let parsed: ConnectionRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn connection_ref_builtin_round_trip() {
    let original = ConnectionRef::Builtin {
        integration: "twilio".into(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let parsed: ConnectionRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn connection_ref_mcp_round_trip() {
    let original = ConnectionRef::Mcp {
        server_id: "obsidian-vault".into(),
        tool_name: Some("note_search".into()),
    };
    let json = serde_json::to_string(&original).unwrap();
    let parsed: ConnectionRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn connection_ref_generic_http_round_trip() {
    let original = ConnectionRef::GenericHttp {
        connection_id: "01F9-aaa".into(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let parsed: ConnectionRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn auth_kind_variants_round_trip() {
    for original in [
        AuthKind::None,
        AuthKind::Bearer,
        AuthKind::Basic,
        AuthKind::ApiKeyHeader {
            name: "X-API-Key".into(),
        },
        AuthKind::QueryParam {
            name: "api_token".into(),
        },
    ] {
        let json = serde_json::to_string(&original).unwrap();
        let parsed: AuthKind = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original, "round-trip failed for {original:?}");
    }
}

#[test]
fn generic_http_connection_round_trip_empty_headers() {
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let original = GenericHttpConnection {
        id: "01F9-aaa".into(),
        name: "my-zapier-hook".into(),
        base_url: "https://hooks.zapier.com/v1".into(),
        auth_kind: AuthKind::Bearer,
        secret_ref: Some(SecretRef {
            name: "zapier_token".into(),
        }),
        default_headers: vec![],
        created_at: now,
        updated_at: now,
    };
    let json = serde_json::to_string(&original).unwrap();
    let parsed: GenericHttpConnection = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn generic_http_connection_round_trip_with_headers() {
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let original = GenericHttpConnection {
        id: "01F9-bbb".into(),
        name: "n8n-cloud".into(),
        base_url: "https://my-n8n.cloud".into(),
        auth_kind: AuthKind::ApiKeyHeader {
            name: "X-N8N-API-Key".into(),
        },
        secret_ref: Some(SecretRef {
            name: "n8n_api_key".into(),
        }),
        default_headers: vec![
            ("Content-Type".into(), "application/json".into()),
            ("X-Source".into(), "openhuman".into()),
        ],
        created_at: now,
        updated_at: now,
    };
    let json = serde_json::to_string(&original).unwrap();
    let parsed: GenericHttpConnection = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn connection_status_round_trip() {
    for original in [
        ConnectionStatus::Connected,
        ConnectionStatus::NotConnected,
        ConnectionStatus::Disabled,
        ConnectionStatus::Error {
            reason: "token expired".into(),
        },
    ] {
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ConnectionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }
}
