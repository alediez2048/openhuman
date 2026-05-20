# ADR-015: Bounded auto-retry on proposal validation failure (3 attempts)

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

The drafting sub-agent produces a `WorkflowProposal` JSON document. Sometimes it fails — invalid JSON, unknown connection names, malformed cron expressions, references to a `NodeKind` not yet available in the current phase. The question is where to handle that failure: surface it to the chat agent (which might or might not retry), make the user re-describe their workflow, or auto-retry transparently.

This is partly an LLM-reliability question and partly a UX question. Forcing the user to retype "I want…" because the agent fumbled JSON braces is unacceptable. Unbounded retries are equally bad — they could spin forever and burn LLM budget. This maps to `requirements.md §8` OQ-19.

## Decision

`proposer::draft_with_retries` validates every proposal returned by the drafting sub-agent and **retries up to 3 total attempts** (1 original + 2 retries) when validation fails, feeding the structured `ProposalValidationError` back into the next attempt's system prompt as context. After 3 failures, return `DraftFailure::ValidationFailedAfterRetries` to the chat agent, which surfaces a *"Drafting failed after 3 attempts. Last error: …"* message to the user. Worst-case latency: 90s (NFR-2.1.4); happy-path: 30s for a single attempt.

## Alternatives considered

**No retry — surface the first failure to the chat agent.** Let the chat agent observe the failure and decide whether to call `workflow_propose_create` again. Rejected because the chat agent doesn't have the structured `ProposalValidationError` context the drafting agent needs — re-asking with the same prompt is unlikely to fix the issue, and the chat agent has no incentive to fix it correctly.

**Unbounded retry until success.** Keep retrying as long as validation fails. Rejected because a structural mismatch (e.g., the user described a Phase 2 trigger that doesn't exist yet) is unrecoverable; spinning forever burns tokens and locks the user out of the chat thread.

**Main-agent-driven retry.** The chat agent receives the error and decides whether to invoke the propose tool again. Rejected because it puts retry logic in the wrong place — `proposer.rs` already has the system prompt, the connections snapshot, and the validator output; passing that context out to the chat agent and back is just an indirection.

## Consequences

### Positive
- Most LLM stumbles (JSON braces, hallucinated connection names, malformed cron) are self-healing — the user sees a slightly longer "Thinking…" indicator and then a valid proposal.
- The retry budget is bounded (3 attempts) — worst-case latency is predictable (NFR-2.1.4: 90s).
- `ProposalValidationError` feedback in the retry prompt teaches the agent what specifically to fix; we measure each variant via `[workflows-validator]` metrics (NFR-2.4.2) and tune prompts based on which errors dominate.

### Negative
- A 3-attempt run worst-case takes 90s of "Thinking…" before failing — long enough that users may abandon the thread. Mitigation: the UI shows the "Thinking…" indicator continuously (NFR-2.1.4).
- Three LLM calls' worth of token cost on retry-heavy proposals. The validator runs in <50ms (NFR-2.1.5) so the cost is the model invocations themselves.

### Neutral
- The drafting sub-agent's `iteration_cap` is independently 6 (FR-1.13.2) — that's tool-call iterations *within* an attempt, distinct from the 3 *attempts* of `draft_with_retries`. The two budgets don't interact.

## Implementation notes

- `src/openhuman/workflows/proposer.rs::draft_with_retries` — full pseudocode in `systemsdesign.md §4.4`.
- Telemetry: `metrics::counter!("workflow_proposal_validation_error", "kind" => e.kind_label()).increment(1)` per retry.
- Integration test (NFR-2.6.6): mock LLM that fails attempts 1 + 2, succeeds attempt 3 — asserts the proposal returns and metrics record the two validation errors.

## Related ADRs

- ADR-019 (`ProposalValidationError` structured variants) — defines the error types this retry loop consumes.
- ADR-007 (Chat as primary creation path) — defines the surrounding flow where this retry happens.
- ADR-009 (Hybrid connection discovery) — the most common retry trigger (`UnknownConnection`).
