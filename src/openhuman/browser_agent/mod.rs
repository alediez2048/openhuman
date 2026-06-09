//! Phase 3 — Browser Agent.
//!
//! CDP-driven browser automation that lets a workflow_node sub-agent
//! drive the user's already-authenticated CEF webview session. The
//! agent exposes a tight three-primitive surface (`browser_observe`
//! / `browser_act` / `browser_extract`) modeled on Stagehand's API;
//! everything below is plumbing.
//!
//! ## Architecture (bottom up)
//!
//! ```text
//! workflow_node sub-agent (F-16, with browser_* in allowlist)
//!     ↓
//! browser_observe / browser_act / browser_extract  (F3-3)
//!     ↓
//! PageSnapshot — DOM + a11y tree → LLM-readable     (F3-2)
//!     ↓
//! CdpSession primitives (click, type, scroll, …)    ← F3-1 (this module)
//!     ↓
//! Existing CEF child-webview infrastructure
//! ```
//!
//! ## Why a fresh core-side module instead of reusing the shell-side one?
//!
//! The Tauri shell already has CDP plumbing at `app/src-tauri/src/cdp/`
//! that the per-provider scanners use (Discord / Slack / WhatsApp /
//! Telegram). That module is provider-aware and lives in the desktop
//! shell — it can't be called from the in-process core where the
//! workflow runtime + LLM tools live.
//!
//! F3-1 builds a fresh, **provider-agnostic, core-side** CDP wrapper
//! at `src/openhuman/browser_agent/cdp/`. It talks to the same CEF
//! debug port (127.0.0.1:19222) as the shell-side module, so both
//! coexist without resource contention.
//!
//! ## What lives here in Phase 3.1
//!
//! - `cdp/` — F3-1 CDP automation primitives (this ticket).
//! - `perceive/` — F3-2 page perception. (Adds in F3-2.)
//! - `registry.rs` — F3-3 per-run SessionRegistry. (Adds in F3-3.)
//! - `preview/` — F3-5 broadcaster for live-preview frames. (Adds in F3-5.)
//! - `safety/` — F3-6 confirmation policy + cost caps. (Adds in F3-6.)
//!
//! See `Automations/Tickets/phase-3-browser-agent/F3-overview.md` for
//! the full thesis + the five forks (runtime / grounding / LLM API /
//! workflow integration / safety) with their locked recommendations.

pub mod cdp;
pub mod perceive;
