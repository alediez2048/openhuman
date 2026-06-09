//! F3-2 — page perception.
//!
//! Turns a live CDP-attached page into a structured [`PageSnapshot`]
//! the LLM can read: a compact list of actionable elements + a
//! trimmed text excerpt. Output stays under ~3000 tokens for 90%+
//! of real-world pages (per the F3-2 ticket budget).
//!
//! The LLM addresses elements by `[N]` — `browser_act("click [3]")`
//! resolves to a CDP click at element id 3's bounds, courtesy of
//! F3-3.
//!
//! ## What lives where
//!
//! - [`snapshot::snapshot`] — main entry. One `Runtime.evaluate` of
//!   the bundled `dom_extractor.js` plus optional accessibility-tree
//!   augmentation (deferred to F3-2 follow-up; landed as scaffold here).
//! - [`elements::ActionableElement`] — one row in the snapshot.
//! - [`render::to_llm_text`] — the `[N] role "label" — attrs` format
//!   F3-3 streams to the LLM.
//!
//! ## Phase 3.1 scope cuts (intentional)
//!
//! - **A11y-tree augmentation** is the F3-2 ticket's section C; F3-2
//!   MVP ships DOM-only and notes the gap. Adding it later is a
//!   single `Accessibility.getFullAXTree` call + a per-element role-
//!   override walk. Documented in `snapshot.rs::snapshot`.
//! - **Iframes** are not recursed (ticket-confirmed Phase 3.1 cut).
//! - **Visual / canvas fallback** lives in F3-7 (vision grounding),
//!   not here.

pub mod elements;
pub mod render;
pub mod snapshot;

#[cfg(test)]
mod tests;

pub use elements::{ActionableElement, ElementRole, ElementState, Viewport};
pub use render::{to_llm_text, DetailTier};
pub use snapshot::{snapshot, PageSnapshot, SnapshotOptions};
