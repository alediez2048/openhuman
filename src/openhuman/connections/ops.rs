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

    // F-19 Part 2: auto-probe the endpoint before save.
    //
    // The user pastes a base URL (e.g. `https://mcp.higgsfield.ai`);
    // they have no way to know whether the MCP JSON-RPC is exposed at
    // `/`, `/mcp`, `/sse`, or `/messages` — every server is different.
    // Pre-F-19 this meant they discovered the right path by tool-call
    // failure later, with the chat agent confabulating the cause. The
    // probe runs an `initialize` request against a candidate sequence
    // and replaces the persisted endpoint with the first that responds.
    // Stdio servers (`command` set) skip the probe entirely.
    let (corrected_endpoint, probe_log) = if endpoint.is_empty() {
        (endpoint, None)
    } else {
        let probed = probe_mcp_endpoint(&endpoint, &auth).await;
        match probed {
            Ok(outcome) => {
                let log_line = if outcome.corrected {
                    format!(
                        "auto-probe corrected MCP endpoint: `{}` → `{}` (HTTP 200 on path `{}` after trying {} candidate(s))",
                        endpoint, outcome.url, outcome.matched_path, outcome.tried.len()
                    )
                } else {
                    format!("auto-probe verified MCP endpoint `{}` (HTTP 200)", outcome.url)
                };
                tracing::info!(target: "connections", "[connections] {log_line}");
                (outcome.url, Some(log_line))
            }
            Err(e) => {
                tracing::warn!(
                    target: "connections",
                    "[connections] mcp auto-probe FAILED for endpoint `{endpoint}`: {e}. \
                     Saving the user-provided endpoint verbatim — tool calls may surface \
                     the actual failure mode at runtime."
                );
                (endpoint, Some(format!("auto-probe failed: {e}")))
            }
        }
    };

    let server = McpServerConfig {
        name: name.clone(),
        endpoint: corrected_endpoint,
        command,
        args: req.args.into_iter().filter(|a| !a.is_empty()).collect(),
        env,
        cwd: req.cwd.filter(|c| !c.trim().is_empty()),
        description: req.description.filter(|d| !d.trim().is_empty()),
        enabled: true,
        timeout_secs: 30,
        auth,
    };
    if let Some(line) = probe_log {
        tracing::debug!(target: "connections", "[connections] {line}");
    }

    persisted.mcp_client.servers.push(server.clone());
    guard_against_stale_session_handle(&persisted, "add_mcp_server").await?;
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

/// F-18 stale-session-handle guard.
///
/// The in-flight `Config` carries a `config_path` captured when the
/// handle was loaded. Between that load and this save the active user
/// can change — via logout/login, `test_reset`, or any flow that
/// rewrites `active_user.toml`. Silently saving against the in-flight
/// handle's `config_path` orphans the write to a stale user dir (the
/// F-18 repro: a Higgsfield MCP server registered under user A while
/// active_user.toml had been re-flipped to user B). Recovering from
/// that takes a `sqlite3` / `grep` excursion the user can't be
/// expected to perform.
///
/// This wrapper re-resolves the active user's `config.toml` path right
/// before the save and delegates to
/// [`guard_against_stale_session_handle_with_active_path`].
async fn guard_against_stale_session_handle(persisted: &Config, op_label: &str) -> Result<()> {
    let active_path = crate::openhuman::config::resolve_active_config_path()
        .await
        .map_err(|e| {
            anyhow!(
                "stale_session_handle: could not resolve active user's config path \
                 while running {op_label} (active_user.toml unreadable: {e}). \
                 Refresh your session and try again."
            )
        })?;
    guard_against_stale_session_handle_with_active_path(persisted, op_label, &active_path)
}

/// Testable core of the F-18 stale-handle guard. Takes the active path
/// explicitly so unit tests can drive both branches without mutating
/// the global `OPENHUMAN_WORKSPACE` env var.
///
/// Returns `Ok(())` when the in-flight Config's `config_path` matches
/// the resolved active-user path. Returns an `anyhow!` error tagged
/// with the stable `stale_session_handle:` prefix otherwise — the
/// frontend (`McpAddModal.tsx`) detects this prefix to render the
/// "session changed since you opened this dialog" inline error
/// instead of the silent wrong-user write that produced the F-18 bug.
fn guard_against_stale_session_handle_with_active_path(
    persisted: &Config,
    op_label: &str,
    active_path: &std::path::Path,
) -> Result<()> {
    if persisted.config_path != active_path {
        return Err(anyhow!(
            "stale_session_handle: in-flight Config was loaded against {} \
             but the active user's config is now at {} (active_user.toml \
             changed between load and save). The {op_label} write was \
             refused to avoid silently orphaning the credentials in a \
             stale user dir. Log out and back in, then retry.",
            persisted.config_path.display(),
            active_path.display(),
        ));
    }
    Ok(())
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
    guard_against_stale_session_handle(&persisted, "remove_mcp_server").await?;
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

// ── F-19 Part 2: MCP endpoint auto-probe ───────────────────────────────

/// Outcome of a successful [`probe_mcp_endpoint`] call.
#[derive(Debug, Clone)]
pub struct McpProbeOutcome {
    /// Full URL that responded successfully (base + matched path).
    pub url: String,
    /// Just the path that matched (e.g. `/mcp`).
    pub matched_path: String,
    /// True when the matched path differed from the user-provided URL —
    /// meaning we auto-corrected. False when the user's exact URL worked.
    pub corrected: bool,
    /// List of paths probed in order. For diagnostics + UI rendering.
    pub tried: Vec<String>,
}

/// Probe a sequence of MCP endpoint paths and return the first that
/// responds with a valid JSON-RPC `initialize` result.
///
/// Probe order:
///   1. The user-provided URL exactly (probe what they typed first;
///      respect explicit paths).
///   2. If the user-provided URL has no path (or just `/`), also try:
///      `<base>/mcp`, `<base>/sse`, `<base>/messages`
///
/// Per-path timeout: 5s. Total probe budget: bounded by the candidate
/// count × 5s.
///
/// Uses the `initialize` JSON-RPC method per the MCP Streamable HTTP
/// spec — anything that responds 200 with a parseable JSON-RPC result
/// containing `protocolVersion` is a real MCP server. 401 from the
/// user-provided URL surfaces specifically (auth issue, not wrong
/// path); 404 falls through to the next candidate.
pub async fn probe_mcp_endpoint(
    user_endpoint: &str,
    auth: &McpAuthConfig,
) -> Result<McpProbeOutcome> {
    let url = url::Url::parse(user_endpoint)
        .with_context(|| format!("not a valid URL: `{user_endpoint}`"))?;
    let user_path = url.path().trim_end_matches('/').to_string();
    let mut base = url.clone();
    base.set_path("");
    let base_str = base.as_str().trim_end_matches('/').to_string();

    // Candidate sequence: user's URL first, then auto-discovery paths
    // ONLY when the user-supplied path was empty / root.
    let mut candidates: Vec<String> = vec![user_endpoint.to_string()];
    if user_path.is_empty() || user_path == "/" {
        for extra in ["/mcp", "/sse", "/messages"] {
            let candidate = format!("{base_str}{extra}");
            if candidate != user_endpoint {
                candidates.push(candidate);
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .context("build probe http client")?;

    let mut auth_failed_url: Option<String> = None;
    let mut last_status: Option<u16> = None;
    let mut tried: Vec<String> = Vec::with_capacity(candidates.len());

    for candidate in candidates.iter() {
        tried.push(candidate.clone());
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "openhuman-mcp-probe", "version": "1.0.0" },
            },
        });
        let req = client
            .post(candidate)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            );
        let req = apply_probe_auth(req, auth);
        let resp = match req.body(body.to_string()).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(
                    target: "connections",
                    "[mcp-probe] {candidate} → request failed: {e}"
                );
                continue;
            }
        };
        let status = resp.status().as_u16();
        last_status = Some(status);
        if status == 401 || status == 403 {
            // Token rejected. The server exists at this path — record
            // for a specific error message and stop trying other paths
            // (a wrong path for a known auth-protected server isn't
            // worth checking other paths against the same token).
            auth_failed_url = Some(candidate.clone());
            tracing::debug!(
                target: "connections",
                "[mcp-probe] {candidate} → HTTP {status} (auth rejected)"
            );
            break;
        }
        if !(200..300).contains(&status) {
            tracing::debug!(
                target: "connections",
                "[mcp-probe] {candidate} → HTTP {status}"
            );
            continue;
        }
        // 2xx — confirm it's actually MCP. Read body, look for the
        // `result.protocolVersion` shape (handles both bare-JSON and
        // SSE `event: message\ndata: {...}` framings).
        let body_text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!(target: "connections", "[mcp-probe] {candidate} → body read failed: {e}");
                continue;
            }
        };
        if parse_initialize_protocol_version(&body_text).is_some() {
            let matched_path = url::Url::parse(candidate)
                .ok()
                .map(|u| u.path().to_string())
                .unwrap_or_else(|| "/".to_string());
            return Ok(McpProbeOutcome {
                url: candidate.clone(),
                matched_path,
                corrected: candidate != user_endpoint,
                tried,
            });
        }
        tracing::debug!(
            target: "connections",
            "[mcp-probe] {candidate} → HTTP {status} but body didn't carry initialize result"
        );
    }

    if let Some(url) = auth_failed_url {
        Err(anyhow!(
            "mcp endpoint at `{url}` exists but rejected the bearer token (HTTP 401/403). \
             Double-check the token value or generate a fresh one."
        ))
    } else {
        Err(anyhow!(
            "no MCP endpoint responded to `initialize` at any candidate path. Tried: [{}]. \
             Last HTTP status: {}. Check the URL and the server's MCP path conventions \
             (Higgsfield = /mcp, GitBook = /~gitbook/mcp, Linear = /sse, etc.)",
            tried.join(", "),
            last_status.map(|s| s.to_string()).unwrap_or_else(|| "none".to_string())
        ))
    }
}

fn apply_probe_auth(
    req: reqwest::RequestBuilder,
    auth: &McpAuthConfig,
) -> reqwest::RequestBuilder {
    match auth {
        McpAuthConfig::None => req,
        McpAuthConfig::BearerToken { token } => req.header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {token}"),
        ),
        McpAuthConfig::Basic { username, password } => {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD
                .encode(format!("{username}:{password}"));
            req.header(
                reqwest::header::AUTHORIZATION,
                format!("Basic {encoded}"),
            )
        }
        McpAuthConfig::Header { name, value } => {
            if let (Ok(n), Ok(v)) = (
                reqwest::header::HeaderName::try_from(name.as_str()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                req.header(n, v)
            } else {
                req
            }
        }
        McpAuthConfig::QueryParam { name, value } => {
            req.query(&[(name.as_str(), value.as_str())])
        }
    }
}

/// Parse a body that may be either bare JSON `{"jsonrpc": "2.0", ...}`
/// OR SSE `event: message\ndata: {...}` (the latter is what Higgsfield
/// returns). Pull out the `result.protocolVersion` if present.
fn parse_initialize_protocol_version(body: &str) -> Option<String> {
    // Try bare JSON first.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body.trim()) {
        if let Some(pv) = v
            .get("result")
            .and_then(|r| r.get("protocolVersion"))
            .and_then(|p| p.as_str())
        {
            return Some(pv.to_string());
        }
    }
    // Try SSE framing: scan for a `data:` line, parse its value as JSON.
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(payload) = trimmed.strip_prefix("data:") {
            let payload = payload.trim();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(pv) = v
                    .get("result")
                    .and_then(|r| r.get("protocolVersion"))
                    .and_then(|p| p.as_str())
                {
                    return Some(pv.to_string());
                }
            }
        }
    }
    None
}

// ── F-18 Part 3: orphan MCP server scanner + migration ─────────────────

/// One MCP server discovered under a user dir that is NOT the currently
/// active user. Used by [`list_mcp_orphans`] / [`migrate_mcp_orphan`]
/// to surface the F-17 / F-18 recovery path: "you have credentials in
/// a previous session's config — restore them with one click."
///
/// The `auth.token` field is REDACTED to `[redacted]` in this view; the
/// real token is read server-side during [`migrate_mcp_orphan`] and
/// never crosses the RPC boundary on the listing path.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct McpOrphanServer {
    /// User-dir name (the SHA-style id under `~/.openhuman/users/`).
    pub source_user_id: String,
    pub name: String,
    pub endpoint: String,
    pub command: String,
    pub args: Vec<String>,
    pub enabled: bool,
    pub timeout_secs: u64,
    /// `"bearer_token"` / `"basic"` / `"header"` / `"none"` — enough for
    /// the UI to render the row, never the actual secret.
    pub auth_kind_label: String,
}

/// Result of [`list_mcp_orphans`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub struct McpOrphanListing {
    pub orphans: Vec<McpOrphanServer>,
    /// Total user-dirs scanned (for diagnostics).
    pub user_dirs_scanned: u32,
    /// True iff the scan was capped at the `MAX_USER_DIRS` limit.
    pub capped: bool,
}

const MAX_USER_DIRS_SCANNED: usize = 10;

/// Scan `<openhuman_root>/users/*/config.toml` for `[[mcp_client.servers]]`
/// entries belonging to non-active users. Returns the orphan inventory
/// (secrets redacted) so the UI can surface a "restore previous-session
/// credentials" banner on `/connections`.
///
/// Production wrapper that resolves the openhuman root + active user
/// from defaults; delegates to [`list_mcp_orphans_at`] which is the
/// testable core.
pub async fn list_mcp_orphans(_config: &Config) -> Result<McpOrphanListing> {
    let root = crate::openhuman::config::default_root_openhuman_dir()?;
    let active_user_id = crate::openhuman::config::read_active_user_id(&root);
    list_mcp_orphans_at(&root, active_user_id.as_deref())
}

/// Testable core of [`list_mcp_orphans`]. Takes the openhuman root +
/// active user id explicitly so unit tests can populate a tempdir with
/// fake user configs and assert on the listing without mutating any
/// global env state.
///
/// Best-effort: per-file parse failures are logged + skipped, not
/// propagated. Capped at [`MAX_USER_DIRS_SCANNED`] orphan-bearing files
/// — anyone with more than that has a config sprawl problem worth
/// surfacing separately.
pub fn list_mcp_orphans_at(
    root: &std::path::Path,
    active_user_id: Option<&str>,
) -> Result<McpOrphanListing> {
    let users_dir = root.join("users");
    let pre_login = crate::openhuman::config::PRE_LOGIN_USER_ID;

    let mut listing = McpOrphanListing::default();
    let read_dir = match std::fs::read_dir(&users_dir) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::debug!(
                target: "connections",
                "[connections] mcp-orphan scan: no users/ dir at {} ({e}); returning empty",
                users_dir.display()
            );
            return Ok(listing);
        }
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let user_id = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Skip the active user (their MCP servers aren't orphans) and
        // the pre-login bucket (it's a baseline, not a previous session).
        if active_user_id == Some(user_id.as_str()) || user_id == pre_login {
            continue;
        }
        listing.user_dirs_scanned += 1;
        if listing.orphans.len() >= MAX_USER_DIRS_SCANNED {
            listing.capped = true;
            tracing::warn!(
                target: "connections",
                "[connections] mcp-orphan scan capped at {MAX_USER_DIRS_SCANNED} \
                 orphan-bearing user dirs; some may be missing from the listing"
            );
            break;
        }

        let config_path = path.join("config.toml");
        let toml_str = match std::fs::read_to_string(&config_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let parsed: toml::Value = match toml::from_str(&toml_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    target: "connections",
                    "[connections] mcp-orphan scan: could not parse {} ({e}); skipping",
                    config_path.display()
                );
                continue;
            }
        };

        let servers = match parsed
            .get("mcp_client")
            .and_then(|v| v.get("servers"))
            .and_then(|v| v.as_array())
        {
            Some(arr) => arr,
            None => continue,
        };
        for server_val in servers {
            let table = match server_val.as_table() {
                Some(t) => t,
                None => continue,
            };
            let name = table.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            let endpoint = table.get("endpoint").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let command = table.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let args: Vec<String> = table
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let enabled = table.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            let timeout_secs = table
                .get("timeout_secs")
                .and_then(|v| v.as_integer())
                .and_then(|n| u64::try_from(n).ok())
                .unwrap_or(30);
            let auth_kind_label = table
                .get("auth")
                .and_then(|v| v.as_table())
                .and_then(|t| t.get("kind"))
                .and_then(|v| v.as_str())
                .unwrap_or("none")
                .to_string();
            listing.orphans.push(McpOrphanServer {
                source_user_id: user_id.clone(),
                name,
                endpoint,
                command,
                args,
                enabled,
                timeout_secs,
                auth_kind_label,
            });
            if listing.orphans.len() >= MAX_USER_DIRS_SCANNED * 4 {
                listing.capped = true;
                break;
            }
        }
    }

    tracing::debug!(
        target: "connections",
        "[connections] mcp-orphan scan: found {} orphan server(s) across {} non-active user dir(s)",
        listing.orphans.len(),
        listing.user_dirs_scanned
    );
    Ok(listing)
}

/// Copy one orphan MCP server from a previous-session user's config
/// into the currently-active user's config. Reads the full server entry
/// (including the bearer token) server-side; the secret never crosses
/// the RPC boundary on the listing path, only here on explicit
/// migration. Does NOT delete from the source — the source user may
/// log back in later and expect their MCP servers intact.
///
/// Re-runs the F-18 stale-session-handle guard before the write so a
/// concurrent active-user flip can't silently land the migrated server
/// in yet another stale config.
pub async fn migrate_mcp_orphan(
    config: &Config,
    source_user_id: &str,
    server_name: &str,
) -> Result<McpServerConfig> {
    if source_user_id.trim().is_empty() {
        return Err(anyhow!("source_user_id is required"));
    }
    if server_name.trim().is_empty() {
        return Err(anyhow!("server_name is required"));
    }
    let root = crate::openhuman::config::default_root_openhuman_dir()?;
    let active_user_id = crate::openhuman::config::read_active_user_id(&root);
    if active_user_id.as_deref() == Some(source_user_id) {
        return Err(anyhow!(
            "source_user_id `{source_user_id}` is the currently-active user — \
             nothing to migrate"
        ));
    }

    let source_path = crate::openhuman::config::user_openhuman_dir(&root, source_user_id)
        .join("config.toml");
    let toml_str = std::fs::read_to_string(&source_path).with_context(|| {
        format!(
            "could not read source config at {} (source_user_id may not exist)",
            source_path.display()
        )
    })?;
    let parsed: toml::Value = toml::from_str(&toml_str)
        .with_context(|| format!("could not parse TOML at {}", source_path.display()))?;
    let servers = parsed
        .get("mcp_client")
        .and_then(|v| v.get("servers"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            anyhow!(
                "no [[mcp_client.servers]] entries at {}",
                source_path.display()
            )
        })?;
    let target = servers
        .iter()
        .find(|s| {
            s.as_table()
                .and_then(|t| t.get("name"))
                .and_then(|v| v.as_str())
                .map(|n| n.eq_ignore_ascii_case(server_name))
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            anyhow!(
                "no mcp server named `{server_name}` at {}",
                source_path.display()
            )
        })?;

    let target_str = toml::to_string(target).context("re-serialize target server entry")?;
    let server: McpServerConfig = toml::from_str(&target_str)
        .context("could not decode source server entry as McpServerConfig — schema drift?")?;

    // Reuse `add_mcp_server` to land it in the active user's config —
    // shares the dedup check, the stale-handle guard, and the
    // ConnectionAdded event publish.
    let req = match server.auth.clone() {
        McpAuthConfig::None => McpAddRequest {
            name: server.name.clone(),
            endpoint: server.endpoint.clone(),
            command: server.command.clone(),
            args: server.args.clone(),
            env: server.env.clone().into_iter().collect(),
            cwd: server.cwd.clone(),
            description: server.description.clone(),
            auth: McpAddAuth::None,
        },
        McpAuthConfig::BearerToken { token } => McpAddRequest {
            name: server.name.clone(),
            endpoint: server.endpoint.clone(),
            command: server.command.clone(),
            args: server.args.clone(),
            env: server.env.clone().into_iter().collect(),
            cwd: server.cwd.clone(),
            description: server.description.clone(),
            auth: McpAddAuth::BearerToken { token },
        },
        McpAuthConfig::Basic { username, password } => McpAddRequest {
            name: server.name.clone(),
            endpoint: server.endpoint.clone(),
            command: server.command.clone(),
            args: server.args.clone(),
            env: server.env.clone().into_iter().collect(),
            cwd: server.cwd.clone(),
            description: server.description.clone(),
            auth: McpAddAuth::Basic { username, password },
        },
        McpAuthConfig::Header { name, value } => McpAddRequest {
            name: server.name.clone(),
            endpoint: server.endpoint.clone(),
            command: server.command.clone(),
            args: server.args.clone(),
            env: server.env.clone().into_iter().collect(),
            cwd: server.cwd.clone(),
            description: server.description.clone(),
            auth: McpAddAuth::Header { name, value },
        },
        McpAuthConfig::QueryParam { .. } => {
            // McpAddAuth doesn't expose QueryParam today (the UI's
            // McpAddModal only exposes None / BearerToken / Basic /
            // Header). Bail with a clear migration error rather than
            // silently dropping the auth — the user can re-create the
            // server through whatever path originally added it.
            return Err(anyhow!(
                "migration of `query_param` auth is not supported via this RPC — \
                 re-create the server in the active workspace manually"
            ));
        }
    };
    add_mcp_server(config, req).await
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
