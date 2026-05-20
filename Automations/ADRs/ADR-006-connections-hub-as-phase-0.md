# ADR-006: Connections Hub ships as Phase 0 prerequisite

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 0
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

OpenHuman currently exposes connections through several disjoint surfaces: `/skills` for Composio, separate UIs for native chat channels, webview accounts hidden in onboarding, native integrations buried in settings, MCP servers in a different settings panel. The agent draws from all of them when reasoning about a request, but the user has no single place to see what's connected and to manage it.

For Workflows to work, the agent's drafting sub-agent needs a definitive snapshot of "what connections does this user have?" — and the user needs a single page to fix `health: NeedsConnections` warnings. This maps to `requirements.md §8` OQ-10.

## Decision

We will ship the **Connections Hub as Phase 0**: an independent PR that lands before any Workflows code, renaming `/skills` to `/connections` and unifying all six connection mechanisms (Composio, Chat Channels, Browser Accounts, Built-in Integrations, MCP Servers, Generic HTTP) into a single page with search, filter chips, and deep-links into the existing per-mechanism setup flows.

## Alternatives considered

**Bundle the Connections refactor into Phase 1.** Land Workflows and Connections Hub together. Rejected because the Phase 1 PR is already large (8 tickets) and the Connections Hub is independently valuable — users benefit from the unified hub even if Workflows never ships. Bundling them also delays Phase 0's value behind Workflows risk.

**Defer the unified hub.** Keep the existing scattered surfaces and have Workflows reason across them ad-hoc. Rejected because (a) it leaves the scattered UX in place permanently, (b) the agent's drafting sub-agent needs a canonical "list my connections" RPC anyway (`connections_list` in `requirements.md §1.12.1`), and (c) Generic HTTP — the substrate for the Phase 2 external-platform bridge — has no other home.

## Consequences

### Positive
- Phase 0 ships a discrete, reviewable PR with clear acceptance criteria (`requirements.md §4`).
- The `connections_list` RPC and `ConnectionRef` discriminated union land first, so Phase 1's drafting sub-agent has a stable contract to depend on.
- Generic HTTP gets a first-class home in time for Phase 2's `http_request` node.

### Negative
- Two PR cycles to ship the first user-visible workflow run. Connections Hub merging is a prerequisite for any Phase 1 code review.
- The `/skills → /connections` rename requires a redirect and an i18n key change — small but pervasive.

### Neutral
- Future connection types (e.g., a Phase 3 "OAuth provider directory") slot into the hub without further refactoring.

## Implementation notes

- New domain at `src/openhuman/connections/` per `CLAUDE.md` layout rule.
- `connections.db` ships in this phase (see ADR-003).
- Frontend pages at `app/src/pages/Connections/`.
- See `systemsdesign.md §11` for the hub architecture.
- Redirects: `/skills → /connections`, `/channels → /connections#channels`.

## Related ADRs

- ADR-003 (Separate SQLite databases) — `connections.db` is shipped in this phase.
- ADR-005 (Hybrid positioning) — Generic HTTP from this phase is the outbound bridge for Phase 2.
- ADR-009 (Hybrid connection discovery) — relies on `connections_list` from this phase.
