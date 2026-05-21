//! `WorkflowHealth` recompute helpers (ADR-017).
//!
//! F-3 replaces the F-2 stub with the real walk against the Phase 0
//! `ConnectionsSnapshot`. `recompute(workflow, snapshot)` returns
//! `Ready` iff every `ConnectionRef` referenced by any node's
//! `allowed_connections` is present in the snapshot AND honestly
//! "connected" per the Phase 0 truth table. Otherwise returns
//! `NeedsConnections { missing }`.
//!
//! Honest-connection truth table (must match the
//! `<ConnectorTile> requireVerification` rules the UI uses):
//! - **Composio / Webview / Built-in:** `status == Connected` is
//!   authoritative. Verification is `None` for these mechanisms; the
//!   underlying probe (Composio API call / cookie probe / session
//!   token) is already strong evidence of liveness.
//! - **Generic HTTP / MCP / Channels:** `status == Connected` is
//!   necessary but NOT sufficient. The verification cache must also
//!   record `VerificationResult::Live` for the connection. A row that
//!   exists in the DB but has never been probed (or whose last probe
//!   failed) counts as missing.

use crate::openhuman::connections::types::{ConnectionRef, ConnectionStatus, ConnectionView};
use crate::openhuman::connections::verification::VerificationResult;
use crate::openhuman::workflows::types::{NodeConfig, Workflow, WorkflowHealth};

/// Thin wrapper around the Phase 0 `Vec<ConnectionView>` that the
/// aggregator returns. Exposes the only operation the workflows domain
/// cares about — `is_connected(&ConnectionRef)` — and applies the
/// Phase 0 honest-connection truth table consistently.
#[derive(Debug, Clone, Default)]
pub struct ConnectionsSnapshot {
    views: Vec<ConnectionView>,
}

impl ConnectionsSnapshot {
    /// Construct a snapshot from the aggregator's output. The vector is
    /// taken by value so callers don't accidentally hold a long-lived
    /// borrow into the aggregator's internal state.
    pub fn new(views: Vec<ConnectionView>) -> Self {
        Self { views }
    }

    /// Construct an empty snapshot. Useful for tests + the bootstrap
    /// path where no connections have been registered yet.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Return `true` iff a connection matching `r#ref` is present in the
    /// snapshot and counts as "live" per the Phase 0 truth table.
    ///
    /// `account_id` / `channel_id` / `tool_name` are treated as
    /// wildcards when the requested ref leaves them empty / null. The
    /// starter templates (F-5) emit refs like
    /// `Webview { provider: "linkedin", account_id: "" }` because they
    /// can't know the user's specific account at bundle time; the
    /// match here lets "any LinkedIn webview the user actually
    /// connected" satisfy the requirement.
    pub fn is_connected(&self, r#ref: &ConnectionRef) -> bool {
        let Some(view) = self.views.iter().find(|v| matches_ref(&v.r#ref, r#ref)) else {
            return false;
        };
        if !matches!(view.status, ConnectionStatus::Connected) {
            return false;
        }
        if requires_verification(r#ref) {
            // For HTTP / MCP / Channels: status alone is "Configured",
            // not "Live". A probe must have recorded `Live`.
            return matches!(
                view.verification.as_ref().map(|v| &v.result),
                Some(VerificationResult::Live)
            );
        }
        // Composio / Webview / Built-in — status is already authoritative.
        true
    }

    /// Total count of connection rows in the snapshot. Exposed for
    /// diagnostics + tests.
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// Is the snapshot empty?
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// Iterate over the underlying views — read-only escape hatch for
    /// callers that need more than `is_connected` (e.g. fuzzy-match
    /// helpers in F-11).
    pub fn views(&self) -> &[ConnectionView] {
        &self.views
    }
}

/// Compute the health for a workflow against the connections snapshot.
///
/// Returns `WorkflowHealth::Ready` when every referenced connection is
/// present-and-live; otherwise `NeedsConnections { missing }` with the
/// exact list of refs that need user attention.
///
/// Sub-50 ms per NFR-2.1.5 — pure Rust, no I/O. The snapshot is
/// captured once by the caller; we just walk it.
pub fn recompute(workflow: &Workflow, snapshot: &ConnectionsSnapshot) -> WorkflowHealth {
    let referenced = referenced_connections(workflow);
    let missing = missing_against(&referenced, snapshot);
    if missing.is_empty() {
        WorkflowHealth::Ready
    } else {
        WorkflowHealth::NeedsConnections { missing }
    }
}

/// Walks every node's `allowed_connections` and returns the deduped,
/// sorted union. Phase 1 only inspects `AgentPrompt` nodes (the only
/// supported `NodeKind`); Phase 2 will extend to `ToolCall`,
/// `HttpRequest`, `ChannelMessage`, etc. — each new variant adds an
/// arm here.
pub fn referenced_connections(workflow: &Workflow) -> Vec<ConnectionRef> {
    let mut out: Vec<ConnectionRef> = Vec::new();
    for node in &workflow.nodes {
        match &node.config {
            NodeConfig::AgentPrompt(cfg) => {
                for r in &cfg.allowed_connections {
                    if !out.contains(r) {
                        out.push(r.clone());
                    }
                }
            } // Phase 2: add ToolCall / HttpRequest / ChannelMessage arms.
        }
    }
    out
}

/// Returns the subset of `refs` that the snapshot does NOT report as
/// connected. Ordering of the return matches `refs`.
pub fn missing_against(
    refs: &[ConnectionRef],
    snapshot: &ConnectionsSnapshot,
) -> Vec<ConnectionRef> {
    refs.iter()
        .filter(|r| !snapshot.is_connected(r))
        .cloned()
        .collect()
}

/// Decide whether a `ConnectionRef` requires a `Verification::Live`
/// signal in addition to `status == Connected`. Mirrors the
/// `requireVerification` prop on the Phase 0 `<ConnectorTile>` so the
/// backend's workflow health matches what the user sees in the UI.
fn requires_verification(r#ref: &ConnectionRef) -> bool {
    matches!(
        r#ref,
        ConnectionRef::GenericHttp { .. }
            | ConnectionRef::Mcp { .. }
            | ConnectionRef::Channel { .. }
    )
}

/// Wildcard-aware comparison between a live connection's ref and the
/// caller's required ref. Returns true when the two refer to the
/// "same" connection per the matching rules below.
///
/// **Matching rules:**
/// - Variant must match (Composio ≠ Channel ≠ Webview ≠ Builtin ≠
///   Mcp ≠ GenericHttp).
/// - For Composio: `toolkit_id` must match exactly; `account_id`
///   matches when the requested ref leaves it empty / None
///   (wildcard) or when both sides are equal.
/// - For Channel: `provider` must match; `channel_id` is wildcard
///   when the requested ref leaves it empty (starter templates use
///   this for "any channel under provider X").
/// - For Webview: `provider` must match; `account_id` is wildcard
///   when empty.
/// - For Mcp: `server_id` must match; `tool_name` is wildcard when
///   None on the requested side.
/// - For Builtin / GenericHttp: full equality (no wildcard slots).
///
/// The wildcard semantics fix the F-5 starter-catalog mismatch
/// where templates ship `{ provider: "linkedin", account_id: "" }`
/// but live connections carry a real `account_id`.
fn matches_ref(live: &ConnectionRef, requested: &ConnectionRef) -> bool {
    use ConnectionRef as R;
    match (live, requested) {
        (
            R::Composio {
                toolkit_id: l_tk,
                account_id: l_aid,
            },
            R::Composio {
                toolkit_id: r_tk,
                account_id: r_aid,
            },
        ) => {
            if l_tk != r_tk {
                return false;
            }
            match r_aid.as_deref() {
                None | Some("") => true,
                Some(want) => l_aid.as_deref() == Some(want),
            }
        }
        (
            R::Channel {
                provider: l_p,
                channel_id: l_cid,
            },
            R::Channel {
                provider: r_p,
                channel_id: r_cid,
            },
        ) => l_p == r_p && (r_cid.is_empty() || l_cid == r_cid),
        (
            R::Webview {
                provider: l_p,
                account_id: l_aid,
            },
            R::Webview {
                provider: r_p,
                account_id: r_aid,
            },
        ) => l_p == r_p && (r_aid.is_empty() || l_aid == r_aid),
        (R::Builtin { integration: l }, R::Builtin { integration: r }) => l == r,
        (
            R::Mcp {
                server_id: l_s,
                tool_name: l_t,
            },
            R::Mcp {
                server_id: r_s,
                tool_name: r_t,
            },
        ) => {
            if l_s != r_s {
                return false;
            }
            match r_t.as_deref() {
                None | Some("") => true,
                Some(want) => l_t.as_deref() == Some(want),
            }
        }
        (R::GenericHttp { connection_id: l }, R::GenericHttp { connection_id: r }) => l == r,
        _ => false,
    }
}
