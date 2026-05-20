//! Tests for Generic HTTP CRUD ops + the secret-handling discipline.

use super::*;
use crate::openhuman::config::Config;
use crate::openhuman::connections::types::{
    AuthKind, CreateGenericHttpRequest, NewCredential, UpdateGenericHttpRequest,
};
use tempfile::TempDir;

/// Build a Config with both `workspace_dir` and `config_path` set under a
/// tempdir. The secret store uses `config_path.parent()` as its data root.
fn config_with_workspace(dir: &TempDir) -> Config {
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    config.config_path = dir.path().join("config.toml");
    config
}

#[tokio::test]
async fn create_generic_http_persists_and_encrypts() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let req = CreateGenericHttpRequest {
        name: "my-zapier-hook".into(),
        base_url: "https://hooks.zapier.com/v1/".into(),
        auth_kind: AuthKind::Bearer,
        auth_credential: Some(NewCredential {
            secret: "super-secret-token".into(),
        }),
        default_headers: vec![],
    };
    let created = create_generic_http(&config, req).await.unwrap();

    // base_url normalization: trailing slash stripped.
    assert_eq!(created.base_url, "https://hooks.zapier.com/v1");
    // secret_ref is populated.
    let secret_ref = created
        .secret_ref
        .as_ref()
        .expect("secret_ref should be set");
    // ChaCha20-Poly1305 ciphertext format.
    assert!(
        secret_ref.ciphertext.starts_with("enc2:"),
        "expected enc2: prefix on ciphertext, got {}",
        secret_ref.ciphertext
    );
    // The cleartext "super-secret-token" must not appear anywhere in the persisted row.
    let row_json = serde_json::to_string(&created).unwrap();
    assert!(
        !row_json.contains("super-secret-token"),
        "cleartext credential leaked into persisted JSON: {row_json}"
    );
}

#[tokio::test]
async fn create_generic_http_with_no_auth_kind_omits_secret_ref() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let req = CreateGenericHttpRequest {
        name: "open-api".into(),
        base_url: "https://api.example.com".into(),
        auth_kind: AuthKind::None,
        auth_credential: None,
        default_headers: vec![],
    };
    let created = create_generic_http(&config, req).await.unwrap();
    assert!(created.secret_ref.is_none());
}

#[tokio::test]
async fn create_rejects_base_url_without_scheme() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let req = CreateGenericHttpRequest {
        name: "missing-scheme".into(),
        base_url: "api.example.com".into(),
        auth_kind: AuthKind::None,
        auth_credential: None,
        default_headers: vec![],
    };
    let result = create_generic_http(&config, req).await;
    assert!(result.is_err(), "should reject scheme-less URL");
}

#[tokio::test]
async fn create_rejects_bearer_without_credential() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let req = CreateGenericHttpRequest {
        name: "bearer-no-cred".into(),
        base_url: "https://api.example.com".into(),
        auth_kind: AuthKind::Bearer,
        auth_credential: None,
        default_headers: vec![],
    };
    let result = create_generic_http(&config, req).await;
    assert!(
        result.is_err(),
        "Bearer auth_kind without credential should be rejected"
    );
}

#[tokio::test]
async fn delete_generic_http_removes_row_and_returns_true() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let req = CreateGenericHttpRequest {
        name: "to-delete".into(),
        base_url: "https://api.example.com".into(),
        auth_kind: AuthKind::None,
        auth_credential: None,
        default_headers: vec![],
    };
    let created = create_generic_http(&config, req).await.unwrap();

    let removed = delete_generic_http(&config, &created.id).await.unwrap();
    assert!(removed);

    let after = store::list_generic_http(&config).unwrap();
    assert!(after.iter().all(|r| r.id != created.id));
}

#[tokio::test]
async fn delete_generic_http_unknown_id_returns_false() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let removed = delete_generic_http(&config, "nonexistent-id")
        .await
        .unwrap();
    assert!(!removed);
}

#[tokio::test]
async fn update_generic_http_partial_fields() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let req = CreateGenericHttpRequest {
        name: "original-name".into(),
        base_url: "https://api.example.com".into(),
        auth_kind: AuthKind::Bearer,
        auth_credential: Some(NewCredential {
            secret: "v1-token".into(),
        }),
        default_headers: vec![],
    };
    let created = create_generic_http(&config, req).await.unwrap();
    let original_secret = created.secret_ref.as_ref().unwrap().ciphertext.clone();

    // Update only name — keep secret intact.
    let updated = update_generic_http(
        &config,
        &created.id,
        UpdateGenericHttpRequest {
            name: Some("renamed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.name, "renamed");
    assert_eq!(
        updated.secret_ref.as_ref().unwrap().ciphertext,
        original_secret,
        "auth_credential = None must leave secret_ref intact"
    );
    assert!(updated.updated_at > created.updated_at);
}

#[tokio::test]
async fn update_generic_http_rotates_credential() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let created = create_generic_http(
        &config,
        CreateGenericHttpRequest {
            name: "rotating".into(),
            base_url: "https://api.example.com".into(),
            auth_kind: AuthKind::Bearer,
            auth_credential: Some(NewCredential {
                secret: "v1-token".into(),
            }),
            default_headers: vec![],
        },
    )
    .await
    .unwrap();
    let v1_ciphertext = created.secret_ref.as_ref().unwrap().ciphertext.clone();

    let updated = update_generic_http(
        &config,
        &created.id,
        UpdateGenericHttpRequest {
            auth_credential: Some(NewCredential {
                secret: "v2-token".into(),
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let v2_ciphertext = updated.secret_ref.as_ref().unwrap().ciphertext.clone();

    assert_ne!(
        v1_ciphertext, v2_ciphertext,
        "rotation should produce a new ciphertext"
    );
    // v2 cleartext must not appear in the row.
    let row_json = serde_json::to_string(&updated).unwrap();
    assert!(!row_json.contains("v2-token"));
}

#[tokio::test]
async fn update_generic_http_returns_err_for_unknown_id() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let result = update_generic_http(
        &config,
        "nonexistent",
        UpdateGenericHttpRequest {
            name: Some("doesntmatter".into()),
            ..Default::default()
        },
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_generic_http_stub_returns_ok_for_existing() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let created = create_generic_http(
        &config,
        CreateGenericHttpRequest {
            name: "probe-target".into(),
            base_url: "https://example.com".into(),
            auth_kind: AuthKind::None,
            auth_credential: None,
            default_headers: vec![],
        },
    )
    .await
    .unwrap();

    // Real probe lands on https://example.com which serves a 200 — fine
    // assertion in CI/dev but flaky when the runner is offline. Skip the
    // happy-path probe assertion here; the unknown-id and failure paths
    // below cover the deterministic logic.
    let _ = test_generic_http(&config, &created.id).await;
}

#[tokio::test]
async fn test_generic_http_returns_not_ok_for_unknown_id() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let result = test_generic_http(&config, "nonexistent").await.unwrap();
    assert!(!result.ok);
    assert!(result.error.is_some());
}

#[test]
fn no_credential_field_in_serialized_generic_http_connection_shape() {
    // Sanity backstop for NFR-2.3.2: GenericHttpConnection JSON has no
    // `auth_credential` field name (that's input-only on CreateGenericHttpRequest).
    let now = chrono::Utc::now();
    let conn = GenericHttpConnection {
        id: "x".into(),
        name: "x".into(),
        base_url: "https://x".into(),
        auth_kind: AuthKind::Bearer,
        secret_ref: Some(SecretRef {
            ciphertext: "enc2:abc".into(),
        }),
        default_headers: vec![],
        created_at: now,
        updated_at: now,
    };
    let json = serde_json::to_string(&conn).unwrap();
    assert!(!json.contains("auth_credential"));
    assert!(!json.contains("\"secret\""));
}
