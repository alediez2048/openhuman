//! F4-9 — approval queue types.
//!
//! `ApprovalEntry` mirrors one row of `approval_queue`. Serialised
//! as JSON for RPC; persisted column-by-column by `store.rs`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Stable id for an approval queue row. UUIDv4 string.
pub type ApprovalId = String;

/// Lifecycle status of an entry.
///
/// `Pending` → `Approved` / `Rejected` → `Sent` / `Failed`.
/// Separated so the UI can render "approved, sending…" while the
/// re-issue executes, then flip to `Sent` or `Failed`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Sent,
    Failed,
}

impl ApprovalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalStatus::Pending => "pending",
            ApprovalStatus::Approved => "approved",
            ApprovalStatus::Rejected => "rejected",
            ApprovalStatus::Sent => "sent",
            ApprovalStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            "sent" => Some(Self::Sent),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// One row in `approval_queue`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApprovalEntry {
    pub id: ApprovalId,
    pub campaign_id: String,
    pub workflow_id: String,
    pub run_id: String,
    pub node_id: String,
    /// Action kind slug, e.g. `"gmail_send"`, `"channel_message"`,
    /// `"http_request"`. Drives the re-issue dispatch path when the
    /// user approves.
    pub action_kind: String,
    /// Recipient identifier surfaced in the UI list view (email
    /// address, phone number, channel id). Free-form per `action_kind`.
    pub target: String,
    /// Full action payload. Edit shape: when approving with edits,
    /// the caller passes the replacement object and the store
    /// swaps `payload_json` before the re-issue runs.
    pub payload: serde_json::Value,
    /// Triggering entity record + agent rationale. Optional — null
    /// when the caller couldn't attach context.
    #[serde(default)]
    pub context: Option<serde_json::Value>,
    pub status: ApprovalStatus,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub decided_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub decided_by: Option<String>,
    /// Set when `status == Failed` so the UI can show the send
    /// error inline.
    #[serde(default)]
    pub error: Option<String>,
}

/// Caller-supplied decision when the user clicks Approve / Reject.
///
/// `Approve` re-issues with the original payload; `Edit` swaps in
/// a new payload then re-issues; `Reject` drops the draft without
/// sending.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Reject { reason: Option<String> },
    Edit { new_payload: serde_json::Value },
}

/// `store::enqueue` input shape. Composes the columns the executor
/// has at intercept time. Cloneable so the ops layer can publish a
/// `DomainEvent::ApprovalEnqueued` carrying the same fields after
/// the store call consumes one copy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnqueueApprovalRequest {
    pub campaign_id: String,
    pub workflow_id: String,
    pub run_id: String,
    pub node_id: String,
    pub action_kind: String,
    pub target: String,
    pub payload: serde_json::Value,
    pub context: Option<serde_json::Value>,
}
