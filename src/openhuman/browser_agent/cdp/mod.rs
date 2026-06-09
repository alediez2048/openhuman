//! F3-1 — CDP automation primitives (Rust core).
//!
//! Reusable, provider-agnostic wrapper around Chrome DevTools Protocol
//! that the rest of Phase 3 builds on. F3-2 page perception calls
//! `evaluate`; F3-3 LLM tools call all of click/type/scroll/screenshot;
//! F3-7 vision fallback calls `screenshot` + `click_at` directly.
//!
//! ## Public surface
//!
//! - [`CdpSession`] — one open page session; primitives live here.
//! - [`CdpTransport`] trait — testability seam (mock impl for unit
//!   tests, [`WsTransport`] for the real CDP WebSocket).
//! - [`BrowserProfile`] — Reuse authenticated webview / Ephemeral
//!   isolated / NamedPersistent — controls session construction.
//! - [`CdpError`] — typed error surface; `?` from primitives bubbles
//!   through.
//!
//! ## What this module does NOT do
//!
//! - **Page perception.** That's F3-2 (`perceive/`).
//! - **LLM-facing tool wrappers.** That's F3-3 (`tools/impl/browser_agent/`).
//! - **Persistent JS injection.** Per CLAUDE.md's CEF ban, the only JS
//!   this module runs is one-off agent-driven `Runtime.evaluate` calls
//!   scoped to a specific task — never `addScriptToEvaluateOnNewDocument`
//!   or init scripts. The safety preamble (F3-6) reinforces the
//!   distinction at the LLM-facing tool layer.

pub mod errors;
pub mod session;
pub mod transport;
pub mod types;

#[cfg(test)]
mod session_tests;

pub use errors::CdpError;
pub use session::CdpSession;
pub use transport::{CdpTransport, MockTransport};
pub use types::{
    BrowserProfile, Cookie, Key, KeyModifiers, MouseButton, Rect, ScreenshotFormat,
    ScreenshotOptions, TypeOptions, WaitOptions, WaitUntil,
};

/// CDP debug port shared with `app/src-tauri/src/cdp/mod.rs::CDP_PORT`.
/// Both surfaces talk to the same CEF instance — kept in sync as a
/// constant in both files so a port change here lands a build-break
/// on the shell side and vice versa.
pub const CDP_PORT: u16 = 19222;
pub const CDP_HOST: &str = "127.0.0.1";
