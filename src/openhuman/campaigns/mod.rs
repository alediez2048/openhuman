//! Campaigns domain — Phase 4 of Workflows & Automations.
//!
//! A **campaign** is the umbrella for long-running, stateful automation
//! that operates on a recordset (vendor outreach across a Sheets row
//! set, content distribution over a month, ads campaign monitoring).
//! It owns N related workflows, a binding to an external recordset
//! (Google Sheets / Attio / future native), throttle + approval
//! policy, and a target outcome.
//!
//! ## Why this exists
//!
//! Phase 1 + 2 workflows are stateless triggered chains — fire once,
//! run, done. Great for "send me a morning digest." Insufficient for:
//! - **Recordset awareness** — "for each of 1,000 vendors in this
//!   sheet, send a personalised outreach"
//! - **Per-record continuity** — "remember what Acme Corp asked last
//!   time and follow up appropriately"
//! - **Cross-workflow coordination** — "outbound at 20/day total
//!   across email + SMS sub-workflows"
//! - **Approval queues** — "draft the email, queue it for my review,
//!   send after I approve"
//!
//! Phase 4's thesis: **a campaign is the unit of business
//! automation**, not a workflow. Workflows are implementation
//! detail underneath.
//!
//! ## Module layout (Phase 4 plan)
//!
//! - [`types`] — the campaign type universe (`Campaign`,
//!   `CampaignStatus`, `EntityRef`, `Throttle`, `ApprovalPolicy`,
//!   `OutcomeSpec`). F4-1 ticket.
//! - `store` — SQLite persistence + migration 008 (F4-2).
//! - `ops` + `rpc` + `schemas` — CRUD + RPC surface (F4-3).
//! - `entity_store` — the pluggable adapter trait + per-provider
//!   adapters (F4-4 .. F4-6).
//! - `executor` — `for_each` iteration + throttle (F4-7 .. F4-8).
//! - `approval` — draft queue + approval gate (F4-9).
//!
//! See `Automations/Tickets/phase-4-campaigns/README.md` for the full
//! architecture rationale (2026-05-26 grill: locked decisions on
//! recordset shape, entity store as trait, chat-primary creation,
//! draft-mode approval policy).

pub mod types;

#[cfg(test)]
mod types_tests;

pub use types::{
    ApprovalPolicy, Campaign, CampaignId, CampaignStatus, EntityRef, OutcomeSpec, TierRule,
    Throttle, ThrottleWindow,
};
