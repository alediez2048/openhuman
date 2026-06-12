//! F3-5 — live-preview broadcaster.
//!
//! Captures CEF screenshot frames on action boundaries (post-
//! write-verb `browser_act`) and publishes them as
//! [`DomainEvent::BrowserPreviewFrame`] events. The future React
//! preview panel + F3-6 chunk 4's confirmation flow both consume
//! these frames as the trust surface — without it, users running a
//! workflow that "logs into my brokerage" have no way to know what
//! actually happened.
//!
//! ## Phase 3.1 / F3-5 chunk 1 scope
//!
//! Rust side only:
//! - [`capture_and_broadcast`] — capture a frame from a `CdpSession`,
//!   convert to base64, publish via the event bus.
//! - Hooked into `BrowserActTool::execute` post-CDP-dispatch (only
//!   the live path; dry-run skips because there's nothing observable
//!   to preview).
//!
//! Frontend consumer (subscriber that bridges events to the
//! `socketService` stream) + the React `BrowserPreviewPanel`
//! component land in F3-5 chunk 2.
//!
//! ## Backpressure
//!
//! [`capture_and_broadcast`] is best-effort: a failed screenshot
//! (CDP timeout, target closed) is logged at `warn!` and swallowed.
//! The agent loop continues. Subscribers see only the next
//! successful frame. The event bus is broadcast-style with a
//! bounded channel — slow subscribers receive `Lagged(n)` and
//! drop intermediate frames; the latest frame still arrives.

pub mod broadcaster;

pub use broadcaster::{capture_and_broadcast, PreviewCaptureOptions};
