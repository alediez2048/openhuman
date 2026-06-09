//! Campaign agent tools (F4-3b).
//!
//! Read-only + propose-only surface registered on the orchestrator's
//! allowlist. Mirrors `tools/impl/workflows/` — the agent introspects
//! campaigns via `campaign_list` / `campaign_get`, then drafts state
//! transitions via `campaign_propose_pause` / `_resume` / `_archive`.
//! The user clicks Apply on the rendered preview card to commit
//! (ADR-012: single mutation boundary).
//!
//! `campaign_propose_create` + `campaign_propose_update` are deferred
//! to F4-3c — they need a full `CampaignDrafter` LLM pipeline
//! mirroring `workflows::proposer`. Substantial enough for their own
//! ticket; not blocked by this surface.
//!
//! The name constants here are the canonical source consumed by the
//! orchestrator allowlist (`agent/agents/orchestrator/agent.toml`
//! `[tools].named`) — keep in sync.

mod entity_schema_inspect;
mod get;
mod list;
mod propose_archive;
mod propose_pause;
mod propose_resume;
mod propose_state;

#[cfg(test)]
mod tests;

pub use entity_schema_inspect::EntitySchemaInspectTool;
pub use get::CampaignGetTool;
pub use list::CampaignListTool;
pub use propose_archive::CampaignProposeArchiveTool;
pub use propose_pause::CampaignProposePauseTool;
pub use propose_resume::CampaignProposeResumeTool;

pub const TOOL_CAMPAIGN_LIST: &str = "campaign_list";
pub const TOOL_CAMPAIGN_GET: &str = "campaign_get";
pub const TOOL_CAMPAIGN_PROPOSE_PAUSE: &str = "campaign_propose_pause";
pub const TOOL_CAMPAIGN_PROPOSE_RESUME: &str = "campaign_propose_resume";
pub const TOOL_CAMPAIGN_PROPOSE_ARCHIVE: &str = "campaign_propose_archive";
pub const TOOL_ENTITY_SCHEMA_INSPECT: &str = "entity_schema_inspect";

/// Every campaign agent tool name registered globally. Used by the
/// allowlist-conformance test to assert the orchestrator's
/// `[tools].named` config carries each entry.
pub const ALL_TOOL_NAMES: &[&str] = &[
    TOOL_CAMPAIGN_LIST,
    TOOL_CAMPAIGN_GET,
    TOOL_CAMPAIGN_PROPOSE_PAUSE,
    TOOL_CAMPAIGN_PROPOSE_RESUME,
    TOOL_CAMPAIGN_PROPOSE_ARCHIVE,
    TOOL_ENTITY_SCHEMA_INSPECT,
];
