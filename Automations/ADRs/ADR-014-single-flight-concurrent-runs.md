# ADR-014: Single-flight concurrent runs; overlapping triggers are dropped

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

A workflow can be triggered while a previous run of the same workflow is still in flight — e.g., a cron trigger that fires every 5 minutes, but the previous run is taking 7. The choices: queue the new trigger to run after the current one, run them in parallel, or drop the new trigger.

Parallel runs of an agent-bearing workflow risk concurrent writes to memory, duplicate side effects (two LinkedIn posts!), and confusing user-facing semantics. Queuing requires bounding the queue depth and answering "what if the queue is full?" Dropping is the simplest invariant and matches user expectations for most workflows. This maps to `requirements.md §8` OQ-18.

## Decision

**At most one in-flight run per workflow.** New triggers that arrive while a run is in progress are **dropped**, not queued. Each drop publishes a `DomainEvent::WorkflowRunSkipped { workflow_id, reason: AlreadyRunning, attempted_trigger_source }`. Cancellation (`workflows_cancel_run`) is **soft** — the cancelled flag is set, the current node completes naturally, subsequent nodes are skipped, status becomes `Cancelled`. On core startup, an **orphan-recovery sweep** marks every `workflow_runs` row with `status = Running` as `Failed { reason: CoreCrashed }`.

## Alternatives considered

**Queued (bounded).** Buffer N pending triggers, fire when in-flight slot opens. Rejected because it adds queue semantics (max depth, FIFO vs LIFO, drop-oldest vs drop-newest) and a queue table — substantial complexity for a Phase 1 feature where the dominant pattern is a slow cron (8am daily, weekly retros) where overlap is rare.

**Fully parallel.** Multiple runs of the same workflow execute concurrently. Rejected because (a) it would let a misconfigured cron (every 1 minute) accumulate dozens of concurrent agent invocations, blowing through LLM token budgets, (b) duplicate side effects are user-visible (the LinkedIn post double-publishes), and (c) memory writes inside `agent_prompt` nodes would race.

## Consequences

### Positive
- One simple invariant — `in_flight: HashMap<WorkflowId, RunId>` — covers the entire concurrency model.
- `WorkflowRunSkipped` events surface in the run-history view as "Skipped (already running)" entries (NFR-2.4.5), making the behavior visible and debuggable.
- Soft cancellation is implementable without aborting LLM calls mid-stream — we just stop scheduling subsequent nodes.
- Orphan sweep ensures that a core crash mid-run leaves the workflow eligible for the next trigger.

### Negative
- Genuine overlapping work is dropped without warning. A user whose cron is set too aggressively will miss runs. The transient toast on manual Run-Now drops + the skipped-row in run history are the only feedback.
- No notion of "this run is more important than the in-flight one; cancel the old one and run me instead." If users ask, we revisit.

### Neutral
- A future per-workflow setting (`concurrency_policy: Drop | Queue { depth }`) is a backwards-compatible addition; today's `Drop` is the default.

## Implementation notes

- `ExecutorState.in_flight` is a `parking_lot::Mutex<HashMap<WorkflowId, RunId>>` (`systemsdesign.md §3.2`).
- `executor::dispatch_run` is the single point of in-flight enforcement.
- Soft-cancellation flag persisted on `workflow_runs` row; checked at each node boundary.
- Orphan sweep in `executor::init`, runs before any scheduler dispatch (FR-1.6.10).
- Test: `executor_tests.rs` simulates parallel triggers and asserts the second publishes `WorkflowRunSkipped` (NFR-2.6.5).

## Related ADRs

- ADR-017 (Workflow health computed field) — `health` is also a gate on dispatch; together they form the dispatch precondition.
