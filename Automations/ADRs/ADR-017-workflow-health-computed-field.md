# ADR-017: `Workflow.health` is a computed-but-persisted field

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

A workflow's "liveness" depends on whether its referenced connections exist and are usable. The list-view UI needs to render a health badge (✓ Ready / ⚠️ Needs X / ❌ Failed) for every row, and the dispatcher needs to gate firing on health. The question is whether to compute `health` on every read (expensive at list-time across N workflows) or persist it (faster reads, but requires a recomputation strategy on state changes).

## Decision

`Workflow` carries a **`health: WorkflowHealth` field that is computed but persisted** in the `workflows.health` JSON column for fast list-view reads. The variants are: `Ready`, `NeedsConnections { missing: Vec<ConnectionRef> }`, `LastRunFailed { run_id, reason }` (Phase 2+), `SessionExpired { connection }` (Phase 2+). Recomputation happens (a) on workflow create, (b) on workflow update, (c) by an event-bus subscriber in `workflows/bus.rs` when `ConnectionAdded` or `ConnectionRemoved` fires. It is **not** recomputed mid-run.

## Alternatives considered

**Compute on every list-view request.** Strip `health` from the schema; compute it for each workflow row at `workflows_list` time by joining against `connections_list`. Rejected because (a) the `/workflows` page must render with N=100 in under 200ms cold per NFR-2.1.1, and a per-row connection-existence check would dominate that budget, (b) the dispatcher would also need to recompute, and (c) the badge would lag behind connection-add events without an event subscription anyway.

**Compute only at run-time (no list-view badge).** Don't surface health in the list view at all. Rejected because the whole point of the list view is "what's running, what's broken, what needs attention" — without the badge, a user can't tell at a glance which workflows are blocked.

## Consequences

### Positive
- List-view rendering reads `health` directly from the column — no joins, no per-row computation.
- The dispatcher can gate on `health == Ready` with a single column check.
- Recomputation is event-driven and lazy — `ConnectionAdded` triggers a bounded `UPDATE` on workflows that reference the changed connection.

### Negative
- The persisted value can drift from the underlying truth if the recomputation logic has a bug — though the subscriber is bounded and covered by tests (NFR-2.6.5).
- Schema carries a JSON column for the variant + payload, which is harder to index than a plain enum tag. We index on `health` (the JSON-extracted discriminator) per the schema in `systemsdesign.md §2.4`.

### Neutral
- The Phase 2+ variants (`LastRunFailed`, `SessionExpired`) are declared in the enum but not populated by Phase 1 code paths.

## Implementation notes

- `WorkflowHealth` enum in `src/openhuman/workflows/types.rs`.
- Persistence: `workflows.health` JSON column with discriminator + payload (`systemsdesign.md §2.4`).
- `workflows/bus.rs` subscribes to `ConnectionAdded` / `ConnectionRemoved`, queries affected workflows, recomputes + persists + publishes `WorkflowHealthChanged`.
- Test: simulated `ConnectionAdded` flips a workflow from `NeedsConnections` to `Ready` (NFR-2.6.5).

## Related ADRs

- ADR-011 (Missing connections save with health flag) — defines the user-facing semantics of `NeedsConnections`.
- ADR-014 (Single-flight concurrent runs) — `health` is a precondition for dispatch alongside the in-flight check.
