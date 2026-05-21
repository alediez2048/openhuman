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
    pub fn is_connected(&self, r#ref: &ConnectionRef) -> bool {
        let Some(view) = self.views.iter().find(|v| &v.r#ref == r#ref) else {
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
