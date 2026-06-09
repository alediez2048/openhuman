//! F3-3 — canonical tool-name constants.
//!
//! Single source of truth consumed by both the per-tool `name()`
//! impls and the orchestrator allowlist test (when these tools land
//! in the chat surface in Phase 3.2).

pub const TOOL_BROWSER_OBSERVE: &str = "browser_observe";
pub const TOOL_BROWSER_ACT: &str = "browser_act";
pub const TOOL_BROWSER_EXTRACT: &str = "browser_extract";

pub const ALL_TOOL_NAMES: &[&str] = &[TOOL_BROWSER_OBSERVE, TOOL_BROWSER_ACT, TOOL_BROWSER_EXTRACT];
