//! F3-3 — LLM-facing browser agent tools.
//!
//! Three tools modeled on Stagehand's act/extract/observe shape:
//!
//! - [`BrowserObserveTool`] — "what's on the page right now?"
//! - [`BrowserActTool`] — "do this thing on the page"
//! - [`BrowserExtractTool`] — "pull these specific values out"
//!
//! Together they're the entire surface the workflow_node sub-agent
//! sees; everything below them (CDP primitives, page perception,
//! session lifecycle) is plumbing.
//!
//! ## Allowlist scope
//!
//! Per the F3-3 ticket, these tools are **NOT** added to the
//! orchestrator's `[tools].named` allowlist in Phase 3.1 — they're
//! workflow-only. F-16's `workflow_node` archetype + F3-4's
//! `BrowserAction` node kind add them per-workflow via
//! `build_node_agent_definition` when a workflow declares
//! browser-capable connections. Phase 3.2 opens them up to the
//! chat surface behind a feature flag.
//!
//! ## What ships in Phase 3.1 (F3-3 MVP)
//!
//! - Fast-path `browser_act` (LLM passes `element_id` + verb).
//! - Read-only `browser_extract` returning labeled element data
//!   from the snapshot (no LLM-based extraction loop yet).
//! - `browser_observe` with three detail tiers.
//!
//! ## Deferred to F3-3 follow-up
//!
//! - **Intent LLM inside `browser_act`** (NL action → primitive
//!   translation via a "fast"-tier LLM call). Today the agent must
//!   pass `element_id` for click/type/select, or use the no-id path
//!   only for navigate/scroll where target selection isn't needed.
//! - **Schema-aware `browser_extract`** (call the LLM with the
//!   snapshot text + schema, return validated JSON).

mod act;
mod constants;
mod extract;
mod observe;

#[cfg(test)]
mod tests;

pub use act::BrowserActTool;
pub use constants::{ALL_TOOL_NAMES, TOOL_BROWSER_ACT, TOOL_BROWSER_EXTRACT, TOOL_BROWSER_OBSERVE};
pub use extract::BrowserExtractTool;
pub use observe::BrowserObserveTool;
