//! F3-6 — browser-agent safety layer.
//!
//! Belt-and-suspenders guardrails on top of the F3-3 tools + F3-4
//! BrowserAction node. The layers:
//!
//! 1. **Safety preamble** ([`preamble`]) — instructional text appended
//!    to the user-message prompt the workflow_node sub-agent reads.
//!    Shapes the LLM's intent (don't type credentials, don't post
//!    without confirmation, stay in allowed_hosts).
//! 2. **Dry-run mode** (tool-level) — when the per-run `RunMeta` sets
//!    `dry_run = true`, the write tools (`browser_act` for now)
//!    short-circuit and return a description of what they WOULD have
//!    done instead of dispatching the CDP primitive. Read-only tools
//!    are unaffected.
//!
//! Layered safety: the preamble shapes intent (LLM is a probability
//! distribution; ~80% of "don't type passwords" violations get caught
//! by the LLM itself refusing); the tool-level enforcement is the
//! backstop for when intent leaks through.
//!
//! ## Phase 3.1 / F3-6 chunk 1 scope
//!
//! Ships items A (preamble) + B (dry-run) from the F3-6 ticket. Items
//! C (cost caps), D (confirmation policy), E (audit log), F (redaction),
//! G (UI panels) are tracked in `F3-6.md` and land in subsequent chunks
//! once F3-5's live preview gives them a UI surface (D/F/G depend on
//! the preview panel for the user-facing confirmation flow).

pub mod audit_log;
pub mod preamble;

pub use audit_log::{list_for_run, write_entry, AuditLogEntry};
pub use preamble::safety_preamble;
