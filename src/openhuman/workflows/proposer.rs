//! Drafting sub-agent + bounded retry loop.
//!
//! F-11 fills this module: `draft_with_retries(description, snapshot,
//! phase, max_attempts=3)` runs the drafting sub-agent against the
//! `workflow_builder.md` system prompt with the connections snapshot
//! inlined. Validation failures append a structured error to the next
//! attempt's prompt (ADR-015). F-12 adds the `_for_update` sibling that
//! inlines the current workflow for diff-style proposals.
