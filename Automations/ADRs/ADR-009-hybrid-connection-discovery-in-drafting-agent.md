# ADR-009: Hybrid connection discovery — inline summary + `connections_list` tool

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

The drafting sub-agent (`proposer.rs`) needs to know which connections the user has so it can propose `required_connections` that actually exist. The choices: (a) inline the full connections list into the system prompt every call, (b) keep the prompt lean and require the agent to call a `connections_list` tool to discover them, or (c) hybrid — a tight summary inlined plus a tool for richer detail.

The tradeoff is between prompt cost (inlining is bigger every call), reliability (will the agent remember to call the tool?), and richness (full detail per connection vs. just toolkit ids). This maps to `requirements.md §8` OQ-13.

## Decision

The drafting sub-agent uses a **hybrid discovery model**. The system prompt (loaded from `src/openhuman/agent/prompts/workflow_builder.md`) inlines a tight summary of the user's connections — e.g., *"You have Composio: gmail, slack, linear. Webview: linkedin, twitter. No Generic HTTP."* The agent also has the `connections_list` tool available for richer per-connection detail when needed (e.g., to inspect Generic HTTP base URLs or specific Composio account scopes).

## Alternatives considered

**Full inline list.** Inline the entire `connections_list` output (with per-connection auth status, scopes, account labels) into every prompt. Rejected because connection counts can be large (Composio alone has ~1000 toolkits, plus channels, webview accounts, integrations, MCP servers); the prompt grows unpredictably with the user's setup, and most of the detail is irrelevant for most proposals.

**Tool-call-only discovery.** Strip connections from the prompt entirely; force the agent to call `connections_list` first thing every time. Rejected because (a) it costs an extra tool round-trip on every proposal even for trivial cases, and (b) LLMs sometimes skip the discovery step and hallucinate connection names — having the summary in the prompt anchors the agent against the user's actual setup.

## Consequences

### Positive
- Cheap by default (small inline summary), rich on demand (tool call for detail).
- Anchors the agent against real connection names — hallucination of unknown connections becomes a validator error (ADR-019 `UnknownConnection`) caught and retried, not a silent bad proposal.
- Same `connections_list` tool that the chat agent uses — one Phase 0 RPC backs both surfaces.

### Negative
- Two sources of truth in the prompt context (summary + potential tool result). The drafting sub-agent must reconcile them; tested by the validator's `UnknownConnection` check.
- The summary is built per-call by `proposer.rs` from a connections snapshot — the snapshot must be consistent (e.g., taken at the start of the drafting attempt, not refreshed mid-iteration).

### Neutral
- The format of the inline summary is prompt-engineering and may evolve based on validation-error telemetry (NFR-2.4.2's `[workflows-proposer]` log prefix).

## Implementation notes

- Summary built in `proposer.rs::build_system_prompt(connections, phase, last_error)`.
- Drafting sub-agent's tool allowlist (per `systemsdesign.md §4.2`): `connections_list`, `workflow_list`, `emit_proposal`.
- Validation hook: `validator::validate` enforces `required_connections ⊆ connections_list` output — `UnknownConnection` errors trigger ADR-015's retry loop.

## Related ADRs

- ADR-015 (Bounded auto-retry on proposal validation failure) — catches `UnknownConnection` hallucinations and retries with structured feedback.
- ADR-016 (Sub-agent tool allowlist) — distinct decision about the `agent_prompt` sub-agent's tools, but shares the connection-discovery primitive.
- ADR-019 (`ProposalValidationError` variants) — `UnknownConnection` is the variant produced by this discovery path's failures.
