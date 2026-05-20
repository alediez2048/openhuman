# ADR-016: `agent_prompt` sub-agent tool allowlist — baseline + allowed_connections + 4 read-only workflow tools

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

An `agent_prompt` node runs the OpenHuman agent inside a workflow step with a user-authored prompt. The node's `allowed_connections` field provides an allowlist of which connection-resolved tools it can call. The question is what *else* the sub-agent has access to: the full main-agent tool surface, a minimal sandbox, or something in between.

This decision determines whether a workflow step can recursively spawn new workflows, mutate existing ones, or call propose tools — none of which we want, because a workflow is meant to be a deterministic-ish, observable unit, not a creator of more workflows. This maps to `requirements.md §8` OQ-20.

## Decision

The `agent_prompt` sub-agent's tool allowlist is **baseline tools + node's `allowed_connections` + the four read-only workflow tools**. Specifically:
- **Baseline:** memory, web_search, time, etc. (the small set always available to OpenHuman agents).
- **Per-node:** the resolved tools for each `ConnectionRef` in `allowed_connections`, filtered against the user's actual connections at run-time.
- **Read-only workflow tools:** `workflow_list`, `workflow_get`, `workflows_list_runs`, `workflows_get_run`.
- **Excluded:** every `workflow_propose_*`, every mutating workflow surface, every non-baseline agent tool.

Implementation: `executor::build_node_agent_definition(allowed_connections)` returns the exact `AgentDefinition.allowed_tools` list.

## Alternatives considered

**Fully sandboxed (no workflow tools at all).** Strip even the four read-only workflow tools. Rejected because legitimate use cases ("In the morning digest, mention which of my workflows ran yesterday") become impossible without the read tools, and they're trivially safe (no side effects).

**Full main-agent surface.** Give the sub-agent everything the main chat agent has, including the propose tools. Rejected because it allows recursive workflow creation — a workflow step that propose-creates more workflows — which is structurally unsound and impossible to reason about. Even with the UI-button-click safety boundary (ADR-012), the propose tools generate output that wastes tokens for no value inside a workflow context.

## Consequences

### Positive
- **Recursion is structurally impossible.** A workflow step cannot create a workflow because the propose tools aren't on the allowlist.
- The allowlist is testable: `executor_tests.rs` asserts `executor::build_node_agent_definition` returns exactly the spec'd allowlist (no propose tools, no mutating tools) — Phase 1 DoD.
- The four read-only workflow tools are useful for reasoning ("am I about to duplicate an existing workflow's behavior?") without enabling any side effects.

### Negative
- The exclusion list has to be maintained alongside the agent-tool registry. A new agent tool added in `src/openhuman/agent/tools/` is implicitly excluded from `agent_prompt` sub-agents unless added to the baseline — a minor governance burden, but conservative-by-default is safe.
- Some legitimate use cases (e.g., "this workflow should send a Slack message via the main chat agent's full Slack tool surface") require adding the connection-resolved tools via `allowed_connections` — slightly more verbose than "just use everything."

### Neutral
- The drafting sub-agent (`proposer.rs`) has a *different* tool allowlist (see ADR-009: `connections_list`, `workflow_list`, `emit_proposal`). The two sub-agents are independent in their sandboxing.

## Implementation notes

- `src/openhuman/workflows/executor.rs::build_node_agent_definition` — returns the full allowlist.
- NFR-2.3.7 codifies the allowlist contract.
- Phase 1 DoD checklist asserts the allowlist matches via `executor_tests.rs`.
- See `systemsdesign.md §3.3` for the per-node executor flow.

## Related ADRs

- ADR-012 (UI-direct mutations, no mutating agent tools) — this ADR applies the same principle inside workflow execution.
- ADR-009 (Hybrid connection discovery) — distinct sub-agent allowlist for the drafting sub-agent.
