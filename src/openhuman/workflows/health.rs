//! `WorkflowHealth` recompute helpers (ADR-017).
//!
//! F-3 fills this module:
//! - `recompute(&Workflow, &ConnectionsSnapshot) -> WorkflowHealth` —
//!   walks each node's `allowed_connections` against the snapshot and
//!   returns `Ready` iff every reference is present AND `Connected`
//!   (with verification respected per the Phase 0 truth table).
//! - `referenced_connections(&Workflow) -> Vec<ConnectionRef>` and
//!   `missing(&[ConnectionRef], &ConnectionsSnapshot)` helpers.
//!
//! F-2 calls a stub here returning `WorkflowHealth::Ready` on create
//! until F-3 lands the real recompute.
