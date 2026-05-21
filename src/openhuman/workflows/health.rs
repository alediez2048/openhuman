//! `WorkflowHealth` recompute helpers (ADR-017).
//!
//! F-2 lands a stub returning `Ready` unconditionally so `ops::create`
//! / `ops::update` have a call site to plug into. F-3 replaces the
//! stub with the real implementation that walks each node's
//! `allowed_connections` against the connections snapshot and returns
//! `NeedsConnections { missing }` when any reference is absent.
//!
//! The signature is intentionally locked here so F-3's body change is a
//! non-breaking swap.

use crate::openhuman::workflows::types::{Workflow, WorkflowHealth};

/// Compute the health for a workflow against the connections snapshot.
///
/// **F-2 stub:** ignores the workflow + snapshot and returns
/// `WorkflowHealth::Ready`. F-3 will walk
/// `node.config.allowed_connections` and return
/// `NeedsConnections { missing }` when any reference is not present
/// or not `Connected` in the snapshot.
///
/// Snapshot is typed as `&()` for now because the typed
/// `ConnectionsSnapshot` shape lands with F-3. Callers in F-2 pass
/// `&()`; F-3 will widen the parameter (and add the snapshot type) in
/// one step.
pub fn recompute(_workflow: &Workflow, _snapshot: &()) -> WorkflowHealth {
    // TODO(F-3): walk allowed_connections, compute NeedsConnections /
    // SessionExpired states against the real connections snapshot.
    WorkflowHealth::Ready
}
