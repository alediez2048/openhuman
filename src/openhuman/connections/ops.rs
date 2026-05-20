//! Pure domain operations for the Connections domain.
//!
//! Phase 0 / P0-3 ships the Generic HTTP CRUD layer + a stub `test_generic_http`.
//! Real HTTP probe (HEAD→OPTIONS→GET against the OpenHuman `reqwest` factory)
//! is deferred to follow-up P0-3a.
//!
//! ## Secret handling discipline
//!
//! Cleartext credentials live only in [`crate::openhuman::connections::types::NewCredential`]
//! during a single create/update call. The value is immediately encrypted via
//! [`crate::openhuman::security::secrets::SecretStore::encrypt`] and persisted
//! as a [`crate::openhuman::connections::types::SecretRef`] (containing the
//! `enc2:<hex>` ciphertext). The cleartext never appears in:
//! - any persisted row,
//! - any RPC response,
//! - any log line (we never log the credential field).
//!
//! See NFR-2.3.2 + ADR-006.

use crate::core::event_bus::{publish_global, DomainEvent};
use crate::openhuman::config::Config;
use crate::openhuman::connections::store;
use crate::openhuman::connections::types::{
    AuthKind, ConnectionRef, CreateGenericHttpRequest, GenericHttpConnection, NewCredential,
    SecretRef, TestProbeResult, UpdateGenericHttpRequest,
};
use crate::openhuman::security::secrets::SecretStore;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use uuid::Uuid;

/// Creates a `SecretStore` rooted at the same data directory `credentials/ops.rs`
/// uses — preserves the existing key-file location.
fn secret_store_for_config(config: &Config) -> SecretStore {
    let data_dir = config
        .config_path
        .parent()
        .map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    SecretStore::new(&data_dir, true)
}

/// Validates + normalizes a `base_url` (no trailing slash, must have scheme).
fn validate_base_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        anyhow::bail!("base_url must start with http:// or https://");
    }
    let normalized = trimmed.trim_end_matches('/').to_string();
    if normalized.is_empty() {
        anyhow::bail!("base_url is empty after normalization");
    }
    Ok(normalized)
}

/// Validates that an `auth_kind` requiring a credential has one supplied.
fn validate_auth_credential(
    auth_kind: &AuthKind,
    credential: Option<&NewCredential>,
) -> Result<()> {
    match auth_kind {
        AuthKind::None => Ok(()),
        _ => match credential {
            Some(c) if !c.secret.trim().is_empty() => Ok(()),
            _ => anyhow::bail!("auth_kind {auth_kind:?} requires a non-empty auth_credential"),
        },
    }
}

/// Creates a new Generic HTTP connection.
///
/// **Ordering contract** (per ADR-006 / NFR-2.3.2):
/// 1. Validate inputs.
/// 2. Encrypt the credential (if any) — produces the `SecretRef`. This is the
///    *only* moment the cleartext exists in memory.
/// 3. Persist the row including the `SecretRef`.
/// 4. Publish `ConnectionAdded`.
///
/// On failure between (2) and (3), no row is written so there's no orphan
/// reference; the encrypted blob is dropped from memory.
pub async fn create_generic_http(
    config: &Config,
    req: CreateGenericHttpRequest,
) -> Result<GenericHttpConnection> {
    let base_url = validate_base_url(&req.base_url)?;
    validate_auth_credential(&req.auth_kind, req.auth_credential.as_ref())?;

    let secret_ref = match req.auth_credential {
        Some(cred) => {
            let store = secret_store_for_config(config);
            let ciphertext = store
                .encrypt(&cred.secret)
                .context("Failed to encrypt Generic HTTP credential")?;
            Some(SecretRef { ciphertext })
        }
        None => None,
    };

    let now = Utc::now();
    let conn = GenericHttpConnection {
        id: Uuid::new_v4().to_string(),
        name: req.name,
        base_url,
        auth_kind: req.auth_kind,
        secret_ref,
        default_headers: req.default_headers,
        created_at: now,
        updated_at: now,
    };

    store::insert_generic_http(config, &conn)?;

    publish_global(DomainEvent::ConnectionAdded {
        connection_ref_json: serde_json::to_value(ConnectionRef::GenericHttp {
            connection_id: conn.id.clone(),
        })
        .unwrap_or(serde_json::Value::Null),
    });

    tracing::info!(
        target: "connections",
        "[connections] generic_http created id={} name={:?}",
        conn.id, conn.name
    );
    Ok(conn)
}

/// Updates an existing Generic HTTP connection. `None`-valued fields keep the
/// existing value. A `Some(NewCredential)` rotates the secret; the old
/// `SecretRef` is dropped from the row (the encrypted blob in `connections.db`
/// is overwritten by the new ciphertext — there is no separate KV store to
/// garbage-collect).
pub async fn update_generic_http(
    config: &Config,
    id: &str,
    req: UpdateGenericHttpRequest,
) -> Result<GenericHttpConnection> {
    let mut existing = store::get_generic_http(config, id)?
        .ok_or_else(|| anyhow!("no generic_http_connections row with id {id}"))?;

    if let Some(name) = req.name {
        existing.name = name;
    }
    if let Some(base_url) = req.base_url {
        existing.base_url = validate_base_url(&base_url)?;
    }
    if let Some(auth_kind) = req.auth_kind {
        existing.auth_kind = auth_kind;
    }
    if let Some(default_headers) = req.default_headers {
        existing.default_headers = default_headers;
    }
    if let Some(cred) = req.auth_credential {
        validate_auth_credential(&existing.auth_kind, Some(&cred))?;
        if !cred.secret.trim().is_empty() {
            let store = secret_store_for_config(config);
            let ciphertext = store
                .encrypt(&cred.secret)
                .context("Failed to encrypt rotated Generic HTTP credential")?;
            existing.secret_ref = Some(SecretRef { ciphertext });
        }
    }
    existing.updated_at = Utc::now();

    store::update_generic_http(config, &existing)?;

    publish_global(DomainEvent::ConnectionUpdated {
        connection_ref_json: serde_json::to_value(ConnectionRef::GenericHttp {
            connection_id: existing.id.clone(),
        })
        .unwrap_or(serde_json::Value::Null),
    });

    tracing::info!(
        target: "connections",
        "[connections] generic_http updated id={}",
        existing.id
    );
    Ok(existing)
}

/// Deletes a Generic HTTP connection by id. Returns `true` when a row was
/// removed, `false` when the id was unknown.
pub async fn delete_generic_http(config: &Config, id: &str) -> Result<bool> {
    let removed = store::delete_generic_http(config, id)?;
    if removed {
        publish_global(DomainEvent::ConnectionRemoved {
            connection_ref_json: serde_json::to_value(ConnectionRef::GenericHttp {
                connection_id: id.to_string(),
            })
            .unwrap_or(serde_json::Value::Null),
        });
        tracing::info!(
            target: "connections",
            "[connections] generic_http deleted id={}",
            id
        );
    }
    Ok(removed)
}

/// Stub for the `connections_test` RPC.
///
/// **Phase 0 / P0-3 placeholder.** Returns `ok: true` if the connection exists,
/// without issuing any HTTP request. Real HEAD→OPTIONS→GET probe (using the
/// OpenHuman `reqwest` client factory + 10-second timeout) is deferred to
/// follow-up P0-3a so this ticket stays bounded.
pub async fn test_generic_http(config: &Config, id: &str) -> Result<TestProbeResult> {
    let exists = store::get_generic_http(config, id)?.is_some();
    if exists {
        tracing::debug!(
            target: "connections",
            "[connections] test_generic_http stub for id={} — TODO P0-3a wire reqwest probe",
            id
        );
        Ok(TestProbeResult {
            ok: true,
            status: None,
            error: Some(
                "probe not yet implemented — connection exists but no network call was made (P0-3a)".into(),
            ),
        })
    } else {
        Ok(TestProbeResult {
            ok: false,
            status: None,
            error: Some(format!("no connection with id {id}")),
        })
    }
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
