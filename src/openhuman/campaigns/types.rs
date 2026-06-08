//! Types for the Campaigns domain (Phase 4 — F4-1).
//!
//! Locks the full type vocabulary in one place so every downstream
//! ticket (F4-2 store, F4-3 RPC, F4-4..F4-6 entity adapters, F4-7..F4-8
//! executor, F4-9 approval queue, F4-10..F4-18 drafter + UI) can
//! consume stable types without redefining them.
//!
//! Wire format: every enum is `#[serde(tag = "type" / "kind")]` so the
//! JSON shape mirrors `WorkflowOrigin` / `WorkflowHealth` etc. from
//! the workflows domain. Storage in F4-2 will round-trip the
//! `Campaign` struct as JSON blobs in dedicated TEXT columns alongside
//! a SQLite row.
//!
//! See `Automations/Tickets/phase-4-campaigns/F4-1.md`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Stable id for a Campaign row. UUIDv4 string (no time prefix —
/// campaigns are coarser-grained than workflow runs so v4 random IDs
/// are clearer to humans than v7 timestamps).
pub type CampaignId = String;

// ── Entity ──────────────────────────────────────────────────────────────

/// A long-running, stateful automation that operates on a recordset.
///
/// Persisted as one row in `campaigns` (F4-2). The `entity_binding`,
/// `throttle`, `approval_policy`, and `target_outcome` fields are
/// round-tripped as JSON in their dedicated TEXT columns.
///
/// `Workflow.campaign_id` is the soft FK from a workflow back to its
/// parent campaign — workflows can also be standalone (Phase 1+2
/// shape), in which case `campaign_id` is `None`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Campaign {
    pub id: CampaignId,
    /// Persisted schema version. Bumped only when the wire format
    /// breaks backwards compatibility — additive Serde changes do
    /// not require a bump.
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub status: CampaignStatus,
    pub entity_binding: EntityRef,
    /// Optional throttle. `None` means "run as fast as triggers fire."
    /// Most outbound campaigns will want one (e.g. "20 emails/day max
    /// across all sub-workflows").
    #[serde(default)]
    pub throttle: Option<Throttle>,
    pub approval_policy: ApprovalPolicy,
    /// Optional target outcome the user wants the campaign to drive
    /// toward (e.g. "20 demo bookings by end of month"). Phase 4
    /// renders this as a progress bar; Phase 5 wires outcome telemetry
    /// to inform proactive surfacing.
    #[serde(default)]
    pub target_outcome: Option<OutcomeSpec>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub last_run_at: Option<DateTime<Utc>>,
}

// ── Lifecycle ───────────────────────────────────────────────────────────

/// Lifecycle states a campaign moves through.
///
/// Legal transitions (enforced in F4-3 ops):
///
/// ```text
///                          ┌────────┐
///                  ┌──────▶│ Active │◀──────┐
///                  │       └────┬───┘       │
///                  │            │           │
///         (enable) │            │ (pause)   │ (resume)
///                  │            ▼           │
///              ┌───┴───┐    ┌───────┐       │
///              │ Draft │    │ Paused│───────┘
///              └───┬───┘    └───┬───┘
///                  │            │
///                  │ (wind_down)│ (wind_down)
///                  ▼            ▼
///             ┌─────────────────────┐
///             │     WoundDown       │
///             └──────────┬──────────┘
///                        │ (archive)
///                        ▼
///                  ┌─────────┐
///                  │ Archived│
///                  └─────────┘
/// ```
///
/// `WoundDown` stops the campaign from accepting NEW work but lets
/// in-flight conversations finish (e.g. a vendor who replied yesterday
/// still gets their follow-up email today). `Archived` is the terminal
/// read-only state — historical view only, no further runs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CampaignStatus {
    /// Newly created, not yet started. No workflows running.
    Draft,
    /// Live — triggers fire, workflows run, records get processed.
    Active,
    /// Temporarily halted by the user. Sub-workflows are disabled
    /// (no scheduling); state preserved for resume.
    Paused,
    /// User explicitly stopped accepting NEW records. In-flight
    /// conversations / replies still flow through reply-handler
    /// workflows so users aren't left mid-conversation.
    WoundDown,
    /// Terminal read-only state. Campaign is preserved for audit but
    /// no workflow under it will ever fire again.
    Archived,
}

impl CampaignStatus {
    /// Can the campaign transition from `self` to `to`?
    ///
    /// Used by F4-3's `update_status` op to enforce the lifecycle
    /// machine. Invalid transitions return a typed error rather than
    /// silently mutating.
    pub fn can_transition_to(self, to: CampaignStatus) -> bool {
        use CampaignStatus::*;
        match (self, to) {
            // Draft → Active (the "enable" path).
            (Draft, Active) => true,
            // Active ⇄ Paused.
            (Active, Paused) | (Paused, Active) => true,
            // Anything except Archived → WoundDown.
            (Draft, WoundDown) | (Active, WoundDown) | (Paused, WoundDown) => true,
            // WoundDown → Archived (the terminal archive).
            (WoundDown, Archived) => true,
            // Self-transition is always legal (idempotent reapply).
            (a, b) if a == b => true,
            _ => false,
        }
    }
}

// ── Entity binding ──────────────────────────────────────────────────────

/// Where the campaign's recordset lives. The agent reads + writes
/// records through the matching `EntityStore` adapter (F4-4..F4-6).
///
/// Two MVP adapters land in F4-5 + F4-6:
/// - `GoogleSheet` — the universal floor. Range syntax matches the
///   Google Sheets API ("Sheet1!A1:Z").
/// - `Attio` — typed CRM records. `object_type` is one of
///   `"people"` / `"companies"` / `"deals"` (extensible).
///
/// Future adapters (Airtable, Notion DB, native `entities.db`) plug
/// in by adding a variant + a `EntityStore` implementation. The trait
/// signature is locked in F4-4 so adding a variant is purely additive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EntityRef {
    /// A range of rows on a Google Sheet. Schema is inferred from the
    /// header row (F4-5).
    GoogleSheet {
        spreadsheet_id: String,
        /// A1-notation range, e.g. `"Vendors!A1:H1000"`. The header
        /// row (typically row 1) defines the schema.
        range: String,
    },
    /// An Attio object instance (People / Companies / Deals). Native
    /// typed schema — no inference needed.
    Attio {
        workspace_id: String,
        /// `"people"`, `"companies"`, `"deals"`, or any custom object
        /// the user has defined in their Attio workspace.
        object_type: String,
    },
}

// ── Throttle ────────────────────────────────────────────────────────────

/// Rate-limit for the campaign across all its sub-workflows. The
/// executor (F4-8) consults this when iterating records to decide how
/// many to act on per window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Throttle {
    /// Maximum number of records the campaign may act on within
    /// `window`. Must be > 0 (validated at create time).
    pub max_per_window: u32,
    pub window: ThrottleWindow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThrottleWindow {
    /// Rolling 24-hour window. "20 per day" = no more than 20 records
    /// in the trailing 24h.
    PerDay,
    /// Rolling 60-minute window.
    PerHour,
    /// Rolling 60-second window. Useful for spike control on tools
    /// that have hard rate limits.
    PerMinute,
}

// ── Approval policy ─────────────────────────────────────────────────────

/// How the campaign handles outbound messages that need a person in
/// the loop. The four modes correspond to the conversation-handling
/// quadrant locked in the 2026-05-26 Phase 4 grill.
///
/// Phase 4 MVP ships only `DraftAndApprove`. The other variants are
/// declared from day one so workflows can persist their intended
/// policy and the executor can reject Phase-4+ modes with a clear
/// `unsupported_policy_in_phase` error until they land.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalPolicy {
    /// Mode (A) — agent auto-sends without user review. Phase 4+
    /// only — too risky for outbound campaigns without trust-building
    /// time. Validator rejects in Phase 4 MVP.
    AutoReply,
    /// Mode (B) — agent drafts every message, queues it for user
    /// review, sends after the user clicks Approve. **The MVP
    /// shape for Phase 4** — proves the platform can handle
    /// conversations safely before unlocking auto-modes.
    DraftAndApprove,
    /// Mode (C) — agent categorises inbound (interested / not
    /// interested / question / spam) and routes by category, but
    /// doesn't compose outbound. Phase 4+.
    Triage,
    /// Mode (D) — per-message policy that selects A/B/C based on
    /// `rules`. The composition pattern; assumes A and C exist.
    /// Phase 5+.
    Tiered { rules: Vec<TierRule> },
}

/// One rule in a [`ApprovalPolicy::Tiered`] policy. Pattern-matches
/// on message metadata (sender, subject, intent) and emits the
/// approval mode to apply. Shape is intentionally loose for now —
/// Phase 5 will land the proper matcher + rule grammar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TierRule {
    /// Free-form predicate string the matcher interprets. Phase 5
    /// formalises this into a proper expression grammar.
    pub r#match: String,
    /// Which underlying mode to apply when the predicate fires.
    /// Mutually exclusive with `Tiered` itself (no recursion).
    pub then: NonTieredApprovalMode,
}

/// Subset of [`ApprovalPolicy`] excluding `Tiered`, used as the
/// "leaf" branch of a Tiered rule. Prevents accidental recursion at
/// the type level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NonTieredApprovalMode {
    AutoReply,
    DraftAndApprove,
    Triage,
}

// ── Outcome spec ────────────────────────────────────────────────────────

// ── Request shapes (F4-3) ───────────────────────────────────────────────

/// Payload for `campaigns_create`. Mirrors the workflows equivalent.
/// Status defaults to `Draft` server-side — the create op fills `id`,
/// `created_at`, `updated_at`, and `schema_version`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateCampaignRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub entity_binding: EntityRef,
    #[serde(default)]
    pub throttle: Option<Throttle>,
    pub approval_policy: ApprovalPolicy,
    #[serde(default)]
    pub target_outcome: Option<OutcomeSpec>,
}

/// Partial update payload — every field is optional. `None` means
/// "leave as-is." Status updates go through `pause` / `resume` /
/// `archive` so the lifecycle invariants are enforced by dedicated
/// transitions (not buried in a generic patch).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CampaignPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub entity_binding: Option<EntityRef>,
    #[serde(default)]
    pub throttle: Option<Option<Throttle>>,
    #[serde(default)]
    pub approval_policy: Option<ApprovalPolicy>,
    #[serde(default)]
    pub target_outcome: Option<Option<OutcomeSpec>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateCampaignRequest {
    pub id: CampaignId,
    pub patch: CampaignPatch,
}

// ── Errors ──────────────────────────────────────────────────────────────

/// Errors the campaigns ops surface. Wire format mirrors
/// `RunNowError` from workflows — each variant carries a stable
/// machine-readable `code()` so the UI can branch on it without
/// parsing the message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CampaignOpError {
    /// Campaign id was not in the store (or was soft-deleted).
    NotFound { id: CampaignId },
    /// A status transition was rejected. `from` and `to` are the
    /// requested transition; the UI uses them to surface a clear
    /// "you can't go from Archived to Active" message.
    InvalidTransition {
        id: CampaignId,
        from: CampaignStatus,
        to: CampaignStatus,
    },
    /// Underlying store / cascade operation failed. `detail` is the
    /// best-effort error message; the UI surfaces it verbatim under
    /// "internal error — please retry."
    Internal { detail: String },
}

impl CampaignOpError {
    /// Stable machine-readable code. The RPC envelope prefixes the
    /// error string with this so the frontend can branch on it.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "not_found",
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::Internal { .. } => "internal",
        }
    }
}

impl std::fmt::Display for CampaignOpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { id } => write!(f, "campaign `{id}` not found"),
            Self::InvalidTransition { id, from, to } => {
                write!(f, "campaign `{id}` cannot transition {from:?} → {to:?}")
            }
            Self::Internal { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for CampaignOpError {}

/// Target outcome the user wants the campaign to drive toward.
/// Optional — campaigns without a measurable outcome (e.g. "remind me
/// daily") leave this as `None`.
///
/// `metric` is a free-form string for Phase 4 (e.g. `"meetings_booked"`,
/// `"replies_received"`, `"deals_closed"`). Phase 5's observability
/// surface formalises the metric catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutcomeSpec {
    /// Free-form metric name. Convention: snake_case.
    pub metric: String,
    /// Target value the user wants to hit (e.g. `20.0` meetings).
    pub target: f64,
    /// Optional deadline. `None` means "ongoing — no deadline."
    #[serde(default)]
    pub deadline: Option<DateTime<Utc>>,
}
