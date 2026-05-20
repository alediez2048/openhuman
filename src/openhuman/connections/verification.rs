//! Process-local verification cache for the Connections Hub.
//!
//! Status semantics on the unified Hub used to be binary
//! (`Connected`/`NotConnected`) which conflated "row exists in DB" with
//! "row actually responds." Per the user-reported IFTTT-with-no-URL bug,
//! this cache adds an evidence layer: each entry tracks the **last time we
//! actually pinged the service** and what happened.
//!
//! The aggregator merges entries from this cache into the
//! `ConnectionView.verification` field on every `connections_list` call.
//! Probe RPCs (`connections_test`, `connections_mcp_test`) write to the
//! cache as a side-effect of running.
//!
//! Storage choice (per ADR-006 + the user's "do the whole plan in
//! sequence" direction): in-memory only. The cache resets on core restart,
//! and the UI shows `Unverified` until the user re-clicks Test. A
//! persistent table can land later; in-memory is sufficient for the
//! Phase 0 trust gap.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Outcome of the last probe we ran against a connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerificationResult {
    /// We pinged the service and got a positive response.
    Live,
    /// We pinged the service and the call failed. `reason` is a short
    /// human-readable string the UI surfaces under the status pill.
    Failed { reason: String },
}

/// Verification metadata attached to a `ConnectionView`. `None` means
/// "never probed in this core session" — the UI shows `Configured` /
/// `Unverified`. `Some(VerificationResult::Live)` is the only state that
/// licenses a green `Connected` badge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Verification {
    pub last_probed_at: DateTime<Utc>,
    pub result: VerificationResult,
}

/// Cache key — every connection kind has a different natural id, so we
/// take the discriminant + the unique identifier as a string.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct VerificationKey {
    pub kind: &'static str,
    pub id: String,
}

impl VerificationKey {
    pub fn generic_http(connection_id: impl Into<String>) -> Self {
        Self {
            kind: "generic_http",
            id: connection_id.into(),
        }
    }
    pub fn mcp(server_id: impl Into<String>) -> Self {
        Self {
            kind: "mcp",
            id: server_id.into(),
        }
    }
    pub fn channel(channel_id: impl Into<String>) -> Self {
        Self {
            kind: "channel",
            id: channel_id.into(),
        }
    }
}

fn cache() -> &'static Mutex<HashMap<VerificationKey, Verification>> {
    static CACHE: OnceLock<Mutex<HashMap<VerificationKey, Verification>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record a successful probe. Overwrites any prior entry.
pub fn record_live(key: VerificationKey) {
    let mut guard = match cache().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.insert(
        key,
        Verification {
            last_probed_at: Utc::now(),
            result: VerificationResult::Live,
        },
    );
}

/// Record a failed probe.
pub fn record_failed(key: VerificationKey, reason: impl Into<String>) {
    let mut guard = match cache().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.insert(
        key,
        Verification {
            last_probed_at: Utc::now(),
            result: VerificationResult::Failed {
                reason: reason.into(),
            },
        },
    );
}

/// Look up the most recent probe outcome for a connection. `None` when
/// the connection has never been probed in this core session.
pub fn lookup(key: &VerificationKey) -> Option<Verification> {
    let guard = match cache().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.get(key).cloned()
}

/// Drop any cached entry for `key` (idempotent). Called when a connection
/// is deleted so a future row with the same id can't inherit a stale probe.
pub fn forget(key: &VerificationKey) {
    let mut guard = match cache().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.remove(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_live_then_lookup_returns_live() {
        let key = VerificationKey::generic_http(format!("test-live-{}", uuid::Uuid::new_v4()));
        record_live(key.clone());
        let got = lookup(&key).expect("entry present");
        assert!(matches!(got.result, VerificationResult::Live));
    }

    #[test]
    fn record_failed_then_lookup_returns_failed_with_reason() {
        let key = VerificationKey::generic_http(format!("test-failed-{}", uuid::Uuid::new_v4()));
        record_failed(key.clone(), "HTTP 503");
        let got = lookup(&key).expect("entry present");
        match got.result {
            VerificationResult::Failed { reason } => assert_eq!(reason, "HTTP 503"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn record_then_forget_clears_entry() {
        let key = VerificationKey::generic_http(format!("test-forget-{}", uuid::Uuid::new_v4()));
        record_live(key.clone());
        assert!(lookup(&key).is_some());
        forget(&key);
        assert!(lookup(&key).is_none());
    }

    #[test]
    fn lookup_unknown_key_returns_none() {
        let key = VerificationKey::mcp(format!("never-probed-{}", uuid::Uuid::new_v4()));
        assert!(lookup(&key).is_none());
    }
}
