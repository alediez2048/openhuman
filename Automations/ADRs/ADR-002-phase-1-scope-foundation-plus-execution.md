# ADR-002: Phase 1 PR scope — foundation + minimum execution + agent-driven creation + starter catalog

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

The Workflows vision spans a Connections Hub, a workflow engine with multiple trigger and node kinds, a chat-driven creation path, a starter-template catalog, and ultimately a visual canvas. Trying to ship that in one PR would be unreviewable; trying to ship it as a stub with no executable hero flow would prove nothing.

The question is where to cut Phase 1: a minimal "just the types and CRUD" PR vs. an end-to-end slice that runs the hero user story (RU-1) for real. This maps to `requirements.md §8` OQ-2.

## Decision

We will ship Phase 1 as **B+: foundation + minimum execution + agent-driven creation + starter catalog**. Concretely: the `workflows/` Rust domain, `workflows.db` persistence, the full Phase 1 RPC + agent-tool surface, `cron` + `manual` triggers, the `agent_prompt` node, the chat-driven `propose → preview → click → mutate` creation path, the RU-1..RU-4 starter catalog, and the `/workflows` UI with list + detail + run history. Visual canvas is deferred to Phase 3 and Phase 2 node kinds wait for the Phase 2 PR.

## Alternatives considered

**Foundation-only stub (Phase 1A).** Ship just the types, schemas, RPC stubs, and an empty `/workflows` page. Rejected because it proves nothing — no hero flow, no validated architecture, no user-visible value, and the agent-driven creation path is the riskiest piece of the whole design (LLM reliability, validator retry loop, propose-then-click contract). Deferring that to Phase 2 means we don't learn what's broken until much later.

**Big-bang Phase 1 with canvas.** Bundle Phases 1+3 into one PR — canvas, branching, all trigger types. Rejected because it's unreviewable in a single PR, and the canvas is *non-blocking* for the product story: chat creation + chat editing already cover what users need. Canvas is a power-user fallback we should only build if users ask for it.

## Consequences

### Positive
- One PR series proves the architecture end-to-end against the hero user story.
- The riskiest piece (agent-driven creation with validation + retry) lands first, where we can learn from it.
- Users get a usable feature, not a stub.

### Negative
- The Phase 1 PR is large — roughly 8 tickets covering Rust domain, RPCs, agent tools, drafting sub-agent + validator, frontend pages, starter catalog, E2E specs.
- We commit to the propose-then-click pattern (ADR-007, ADR-010, ADR-012) before we have user telemetry to validate it.

### Neutral
- The Phase 2 cut becomes "expand trigger + node taxonomy + interop bridge" rather than "add execution at all" — a cleaner second chapter.

## Implementation notes

- Per-ticket DoD in `Tickets/phase-1-foundation/F-*.md`.
- Acceptance criteria checklist in `requirements.md §5`.
- Coverage gate: ≥ 80% on changed lines per `CLAUDE.md` merge gate.

## Related ADRs

- ADR-006 (Connections Hub as Phase 0) — explicit prerequisite, ships first.
- ADR-007 (Chat as primary creation path) — the hero interaction this scope is built around.
- ADR-008 (Starter templates as read-only catalog) — bundled into Phase 1, not deferred.
