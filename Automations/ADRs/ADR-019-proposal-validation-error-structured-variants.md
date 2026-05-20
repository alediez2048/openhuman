# ADR-019: `ProposalValidationError` is a structured enum with explicit variants

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

The drafting sub-agent's proposals fail validation in distinct ways: invalid JSON, references to a connection the user doesn't have, a `NodeKind` not yet available in the current phase, a malformed cron expression, an edge pointing to a nonexistent node, a missing required field. Each of these has a different "fix me" instruction for the retry attempt, and each is a useful telemetry signal for prompt tuning.

The choice is between an opaque error string ("validation failed: …") that the retry prompt just dumps back in, or a structured enum where each variant carries its own context.

## Decision

`ProposalValidationError` is a **structured enum** with explicit variants, each carrying the context relevant to its failure mode:

```rust
pub enum ProposalValidationError {
    JsonParse           { reason: String },
    UnknownConnection   { r#ref: ConnectionRef, candidates: Vec<ConnectionRef> },
    UnsupportedNodeKind { kind: NodeKind, phase: u32 },
    InvalidCron         { expr: String, parse_error: String },
    EdgeIntegrity       { from: NodeId, to: NodeId, reason: String },
    MissingRequiredField{ field: &'static str },
}
```

Each variant has a `kind_label()` method for `metrics::counter!("workflow_proposal_validation_error", "kind" => …)` (per `systemsdesign.md §4.4`).

## Alternatives considered

**Opaque string errors.** Single `ValidationError(String)` variant; the retry prompt just gets the string. Rejected because (a) telemetry can't segment by failure mode without parsing the string, (b) `UnknownConnection` benefits from carrying `candidates` (likely user-meant connections) which the retry prompt can use to nudge the agent toward correct names, and (c) structured variants make `validator.rs` unit tests cleaner — one test per variant per NFR-2.6.5.

**Validation-by-LLM.** Use a small classifier model to check proposals. Rejected because (a) the structural checks (JSON parse, cron parse, edge integrity, connection lookup) are deterministic and faster than an LLM call, and (b) validation latency must stay under 50ms (NFR-2.1.5), which a model call cannot guarantee.

## Consequences

### Positive
- Telemetry per variant — we learn which failure modes dominate and can tune the prompt accordingly. E.g., if `UnknownConnection` is 70% of failures, the connection-summary inlining (ADR-009) needs adjustment.
- Retry prompts are surgical — `UnknownConnection { candidates }` becomes *"Did you mean `spotify` instead of `spotify-pro`?"* in the retry attempt.
- `validator.rs` has one test per variant (NFR-2.6.5) — coverage is mechanical.

### Negative
- Adding a new variant requires touching the enum + the validator + the retry-prompt formatter + the telemetry label set. Small surface, but mechanical.
- The `candidates` field on `UnknownConnection` requires computing a fuzzy match against the user's actual connections — that's a small additional cost in the validator.

### Neutral
- The variant set is documented in FR-1.13.10. New phases (Phase 2's `webhook`, `composio_event` triggers) may add new failure modes — extend the enum.

## Implementation notes

- `ProposalValidationError` in `src/openhuman/workflows/types.rs`.
- Validator: `src/openhuman/workflows/validator.rs::validate(proposal, connections, phase)`.
- Each variant has a `kind_label()` for metrics.
- Retry-prompt formatter in `proposer::build_system_prompt` includes the structured error in the "PREVIOUS ATTEMPT FAILED" section (`systemsdesign.md §4.4`).
- Test (NFR-2.6.5): one test per variant produces the expected error from a crafted bad proposal.

## Related ADRs

- ADR-015 (Bounded auto-retry on proposal validation failure) — consumes these errors as retry feedback.
- ADR-009 (Hybrid connection discovery) — `UnknownConnection` is the variant produced when the agent hallucinates a connection name.
