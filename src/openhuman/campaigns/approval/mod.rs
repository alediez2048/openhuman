//! F4-9 — approval queue for `ApprovalPolicy::DraftAndApprove`.
//!
//! Outbound externally-visible actions in a campaign with draft-and-
//! approve policy don't fire directly; they land here. The user
//! reviews via `/approvals` and approves/rejects/edits. Approval
//! triggers a re-issue path that fires the action with the
//! (possibly edited) payload.
//!
//! The conversation-handling MVP from the 2026-05-26 grill —
//! agents may DRAFT but not SEND autonomously until trust is
//! proven.

pub mod ops;
pub mod store;
pub mod types;

#[cfg(test)]
mod tests;

pub use types::{ApprovalDecision, ApprovalEntry, ApprovalStatus, EnqueueApprovalRequest};
