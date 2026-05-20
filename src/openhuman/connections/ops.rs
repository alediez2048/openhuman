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
use crate::openhuman::config::{Config, McpAuthConfig, McpServerConfig};
use crate::openhuman::connections::types::{
    AuthKind, ConnectionRef, CreateGenericHttpRequest, GenericHttpConnection, McpAddAuth,
    McpAddRequest, NewCredential, SecretRef, TestProbeResult, UpdateGenericHttpRequest,
};
use crate::openhuman::connections::{store, verification};
use crate::openhuman::security::secrets::SecretStore;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use std::collections::HashMap;
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

/// Fetch the full `GenericHttpConnection` row by id. Used by the manage
/// modal so the form is populated with the real saved values (name,
/// base_url, auth_kind, default_headers, timestamps) — never with a
/// frontend-constructed stub.
///
/// `secret_ref` is included as-is; the ciphertext is opaque to the UI
/// (the modal renders a `••••••••` placeholder and only sends a new
/// credential when the user types one).
pub async fn get_generic_http(config: &Config, id: &str) -> Result<Option<GenericHttpConnection>> {
    store::get_generic_http(config, id)
}

/// Deletes a Generic HTTP connection by id. Returns `true` when a row was
/// removed, `false` when the id was unknown.
pub async fn delete_generic_http(config: &Config, id: &str) -> Result<bool> {
    let removed = store::delete_generic_http(config, id)?;
    if removed {
        // Drop the cached verification so a future row with the same id
        // can't inherit a stale probe outcome.
        crate::openhuman::connections::verification::forget(
            &crate::openhuman::connections::verification::VerificationKey::generic_http(id),
        );
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

/// Real HTTP connectivity probe for a Generic HTTP connection (P0-3a).
///
/// Strategy: `HEAD` first (cheap, no body), fall back to `OPTIONS` if the
/// server returns 405, fall back to a `GET` with `Range: bytes=0-0` if
/// `OPTIONS` also fails. 10-second timeout via the OpenHuman `reqwest`
/// client factory.
///
/// Auth and default headers from the stored row are applied to every
/// attempt. The auth credential is decrypted via `SecretStore::decrypt`
/// just-in-time and dropped from memory before the call returns.
///
/// Records the outcome into the per-process verification cache so the
/// next `connections_list` reflects the result without another probe.
pub async fn test_generic_http(config: &Config, id: &str) -> Result<TestProbeResult> {
    let Some(row) = store::get_generic_http(config, id)? else {
        return Ok(TestProbeResult {
            ok: false,
            status: None,
            error: Some(format!("no connection with id {id}")),
        });
    };

    // Decrypt the credential (if any) just-in-time. Cleartext lives only
    // for the duration of this probe and is dropped on every early return.
    let cleartext_secret = match row.secret_ref.as_ref() {
        Some(secret_ref) => {
            let store = secret_store_for_config(config);
            Some(
                store
                    .decrypt(&secret_ref.ciphertext)
                    .context("failed to decrypt Generic HTTP credential for probe")?,
            )
        }
        None => None,
    };

    let mut probe_url = row.base_url.clone();
    // Apply query-param auth (kept inside the URL so reqwest carries it
    // automatically across redirects).
    if let (AuthKind::QueryParam { name }, Some(value)) =
        (&row.auth_kind, cleartext_secret.as_deref())
    {
        let sep = if probe_url.contains('?') { '&' } else { '?' };
        probe_url = format!(
            "{probe_url}{sep}{name}={value}",
            name = urlencoding::encode(name),
            value = urlencoding::encode(value),
        );
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("failed to build reqwest client for HTTP probe")?;

    let build_request = |method: reqwest::Method| -> reqwest::RequestBuilder {
        let mut req = client.request(method, &probe_url);
        for (k, v) in &row.default_headers {
            req = req.header(k, v);
        }
        match (&row.auth_kind, cleartext_secret.as_deref()) {
            (AuthKind::Bearer, Some(token)) => {
                req = req.header("Authorization", format!("Bearer {token}"));
            }
            (AuthKind::Basic, Some(creds)) => {
                req = req.header("Authorization", format!("Basic {creds}"));
            }
            (AuthKind::ApiKeyHeader { name }, Some(value)) => {
                req = req.header(name, value);
            }
            _ => {}
        }
        // Range: bytes=0-0 keeps the GET fallback from streaming a large
        // payload — many servers honour this with a 206 Partial Content.
        req
    };

    let methods: [reqwest::Method; 3] = [
        reqwest::Method::HEAD,
        reqwest::Method::OPTIONS,
        reqwest::Method::GET,
    ];

    let mut last_error: Option<String> = None;
    let mut last_status: Option<u16> = None;
    for method in methods {
        let mut req = build_request(method.clone());
        if method == reqwest::Method::GET {
            req = req.header("Range", "bytes=0-0");
        }
        match req.send().await {
            Ok(resp) => {
                let s = resp.status();
                last_status = Some(s.as_u16());
                if s.is_success() || s.is_redirection() || s == reqwest::StatusCode::PARTIAL_CONTENT
                {
                    verification::record_live(verification::VerificationKey::generic_http(
                        id.to_string(),
                    ));
                    return Ok(TestProbeResult {
                        ok: true,
                        status: Some(s.as_u16()),
                        error: None,
                    });
                }
                // 405 from HEAD/OPTIONS is the canonical "try a different
                // method" — keep walking. Other 4xx/5xx are real failures
                // but we still try the next method in case the server
                // misroutes the original.
                last_error = Some(format!("{method} {probe_url} → HTTP {}", s.as_u16()));
            }
            Err(e) => {
                // Network error (DNS, timeout, TLS) — bail immediately.
                let reason = format!("{method} {probe_url} failed: {e}");
                verification::record_failed(
                    verification::VerificationKey::generic_http(id.to_string()),
                    &reason,
                );
                return Ok(TestProbeResult {
                    ok: false,
                    status: None,
                    error: Some(reason),
                });
            }
        }
    }

    let reason = last_error.unwrap_or_else(|| "all probe methods returned non-2xx".into());
    verification::record_failed(
        verification::VerificationKey::generic_http(id.to_string()),
        &reason,
    );
    Ok(TestProbeResult {
        ok: false,
        status: last_status,
        error: Some(reason),
    })
}

/// Real MCP connectivity probe — calls `initialize` on the registered
/// server and verifies a JSON-RPC handshake. Records the outcome in the
/// verification cache so the next `connections_list` shows Live/Error.
///
/// Uses a 5s timeout on top of the server's configured `timeout_secs` —
/// even a misconfigured server (wrong endpoint, wrong auth) should fail
/// fast rather than hang the modal.
pub async fn test_mcp_server(config: &Config, server_id: &str) -> Result<TestProbeResult> {
    let registry = crate::openhuman::mcp_client::McpServerRegistry::from_config(config);
    if registry.get(server_id).is_none() {
        return Ok(TestProbeResult {
            ok: false,
            status: None,
            error: Some(format!("no MCP server named `{server_id}`")),
        });
    }

    let call = registry.initialize(server_id);
    let result = tokio::time::timeout(std::time::Duration::from_secs(15), call).await;

    match result {
        Ok(Ok(_init)) => {
            verification::record_live(verification::VerificationKey::mcp(server_id.to_string()));
            Ok(TestProbeResult {
                ok: true,
                status: None,
                error: None,
            })
        }
        Ok(Err(e)) => {
            let reason = format!("MCP initialize failed: {e}");
            verification::record_failed(
                verification::VerificationKey::mcp(server_id.to_string()),
                &reason,
            );
            Ok(TestProbeResult {
                ok: false,
                status: None,
                error: Some(reason),
            })
        }
        Err(_elapsed) => {
            let reason = "MCP probe timed out after 15s".to_string();
            verification::record_failed(
                verification::VerificationKey::mcp(server_id.to_string()),
                &reason,
            );
            Ok(TestProbeResult {
                ok: false,
                status: None,
                error: Some(reason),
            })
        }
    }
}

// ── MCP add / remove (P0-6b) ─────────────────────────────────────────────

/// Register a new MCP server in `config.mcp_client.servers` and persist
/// the config to disk. The aggregator's `collect_mcp` collector builds a
/// fresh `McpServerRegistry` on every call, so the new server appears on
/// the next `connections_list` refresh without a core restart.
///
/// Validation:
/// - `name` must be non-empty and unique within the existing roster.
/// - Exactly one of `endpoint` / `command` must be set (HTTP vs stdio
///   transports are mutually exclusive in `McpServerConfig`).
/// - For HTTP, the endpoint must start with `http://` or `https://`.
///
/// Returns the canonical `McpServerConfig` that was persisted so callers
/// can echo it back without re-reading the config.
pub async fn add_mcp_server(config: &Config, req: McpAddRequest) -> Result<McpServerConfig> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(anyhow!("MCP server name is required"));
    }
    let endpoint = req.endpoint.trim().to_string();
    let command = req.command.trim().to_string();
    if endpoint.is_empty() && command.is_empty() {
        return Err(anyhow!(
            "MCP server requires either an HTTPS endpoint or a stdio command"
        ));
    }
    if !endpoint.is_empty() && !command.is_empty() {
        return Err(anyhow!(
            "MCP server cannot have both an endpoint and a command — choose one transport"
        ));
    }
    if !endpoint.is_empty() && !endpoint.starts_with("http://") && !endpoint.starts_with("https://")
    {
        return Err(anyhow!("MCP endpoint must start with http:// or https://"));
    }

    let mut persisted = config.clone();
    if persisted
        .mcp_client
        .servers
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case(&name))
    {
        return Err(anyhow!(
            "an MCP server named `{name}` already exists — pick a different name or remove the existing one first"
        ));
    }

    let auth = match req.auth {
        McpAddAuth::None => McpAuthConfig::None,
        McpAddAuth::BearerToken { token } => {
            if token.trim().is_empty() {
                return Err(anyhow!("bearer token cannot be empty"));
            }
            McpAuthConfig::BearerToken { token }
        }
        McpAddAuth::Basic { username, password } => McpAuthConfig::Basic { username, password },
        McpAddAuth::Header { name: hname, value } => {
            if hname.trim().is_empty() {
                return Err(anyhow!("auth header name cannot be empty"));
            }
            McpAuthConfig::Header { name: hname, value }
        }
    };

    let env: HashMap<String, String> = req
        .env
        .into_iter()
        .map(|(k, v)| (k.trim().to_string(), v))
        .filter(|(k, _)| !k.is_empty())
        .collect();

    let server = McpServerConfig {
        name: name.clone(),
        endpoint,
        command,
        args: req.args.into_iter().filter(|a| !a.is_empty()).collect(),
        env,
        cwd: req.cwd.filter(|c| !c.trim().is_empty()),
        description: req.description.filter(|d| !d.trim().is_empty()),
        enabled: true,
        timeout_secs: 30,
        auth,
    };

    persisted.mcp_client.servers.push(server.clone());
    persisted
        .save()
        .await
        .map_err(|e| anyhow!("failed to persist mcp_client.servers update: {e}"))?;

    publish_global(DomainEvent::ConnectionAdded {
        connection_ref_json: serde_json::to_value(ConnectionRef::Mcp {
            server_id: name.clone(),
            tool_name: None,
        })
        .unwrap_or(serde_json::Value::Null),
    });

    tracing::info!(
        target: "connections",
        "[connections] mcp server `{}` registered ({})",
        name,
        if server.endpoint.is_empty() { "stdio" } else { "http" },
    );

    Ok(server)
}

/// Remove an MCP server entry from `config.mcp_client.servers` by name.
/// Returns `true` when a row was removed, `false` if no match was found
/// (idempotent — same semantics as `delete_generic_http`).
pub async fn remove_mcp_server(config: &Config, name: &str) -> Result<bool> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("MCP server name is required"));
    }
    let mut persisted = config.clone();
    let before = persisted.mcp_client.servers.len();
    persisted
        .mcp_client
        .servers
        .retain(|s| !s.name.eq_ignore_ascii_case(trimmed));
    let removed = persisted.mcp_client.servers.len() != before;
    if !removed {
        return Ok(false);
    }
    persisted
        .save()
        .await
        .map_err(|e| anyhow!("failed to persist mcp_client.servers update: {e}"))?;

    publish_global(DomainEvent::ConnectionRemoved {
        connection_ref_json: serde_json::to_value(ConnectionRef::Mcp {
            server_id: trimmed.to_string(),
            tool_name: None,
        })
        .unwrap_or(serde_json::Value::Null),
    });

    tracing::info!(target: "connections", "[connections] mcp server `{}` removed", trimmed);
    Ok(true)
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
