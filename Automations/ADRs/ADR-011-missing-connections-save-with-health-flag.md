# ADR-011: Save workflows with missing connections, gate via health flag

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

When a user describes a workflow that references connections they haven't set up yet (e.g., "every retweet → LinkedIn post" but the user hasn't connected Twitter or LinkedIn), the drafting sub-agent will produce a valid `WorkflowProposal` but with `missing_connections` non-empty. The question: should Save be blocked, should the UI walk the user through inline OAuth, or should Save just persist the workflow in a degraded state?

This maps to `requirements.md §8` OQ-15.

## Decision

**Save still works.** The workflow persists with `health: NeedsConnections { missing }`. The list-view card surfaces ⚠️ "Needs Twitter, LinkedIn" as a health badge. The enable/disable toggle is **disabled** in this state, with a tooltip directing the user to `/connections` to wire up the missing pieces. Once the user adds a missing connection, a `ConnectionAdded` event triggers the `workflows/bus.rs` subscriber to recompute health, flip the workflow to `Ready`, and re-enable the toggle.

## Alternatives considered

**Block Save until all connections exist.** The Save button is greyed out if `missing_connections` is non-empty. Rejected because it forces the user to context-switch out of chat, set up connections one by one, then return and re-describe the workflow. It also throws away the agent's draft, which represents real user intent.

**Inline-connect (in-card OAuth flows).** The preview component embeds the OAuth setup flow for missing connections. Rejected because (a) OAuth flows are already complex per-mechanism (Composio uses one pattern, webview accounts another, MCP servers a third) — embedding them in a chat-rich-message component would duplicate substantial UI surface from `/connections`, and (b) it conflates "describe the workflow" with "set up your credentials" into one action, which violates the single-responsibility shape of the preview component.

## Consequences

### Positive
- The user's intent is preserved — the proposal lands in `/workflows` as a saved artifact even if not yet fireable.
- The connection-setup step happens on the dedicated Connections page, where the per-mechanism flows already live.
- Health recomputation via `ConnectionAdded` / `ConnectionRemoved` is event-driven and lazy — no polling.
- This is what users intuitively expect: "save the thing, I'll wire it up after."

### Negative
- A workflow can exist in a non-fireable state indefinitely. Run history may have a long lag between create and first-run.
- Requires the `WorkflowHealth` enum + the `workflows/bus.rs` subscriber + the toggle-disabled UI state — three artifacts to keep in sync.

### Neutral
- The cron scheduler must check `health == Ready` before firing — a workflow with `enabled = true` but `health = NeedsConnections` does NOT fire (FR-1.4.1.3).
- The "Add & Enable" button on starter-catalog cards has the same semantics: the workflow is created enabled, but if connections are missing, the scheduler still skips it.

## Implementation notes

- `WorkflowHealth` enum in `src/openhuman/workflows/types.rs`.
- `workflows/bus.rs` subscribes to `ConnectionAdded` / `ConnectionRemoved` and recomputes health per FR-1.1.8.
- Toggle component (`WorkflowEnableToggle.tsx`) disables when `health != Ready`, shows tooltip per FR-1.2.4.
- Scheduler guard: `cron::JobType::WorkflowTrigger` dispatcher checks health before calling `executor::dispatch_run`.

## Related ADRs

- ADR-017 (Workflow health computed field) — defines the field this decision drives.
- ADR-013 (Webhook escape hatch) — orthogonal case for unknown triggers; doesn't suffer from missing-connection issues because webhook triggers have no required connection.
