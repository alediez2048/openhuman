# ADR-012: UI-direct mutations; the agent has zero mutating workflow tools

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1+
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

The chat agent participates in workflow creation, edit, and delete by emitting preview components. The question is what actually invokes the `workflows_create` / `_update` / `_delete` RPC on the user's behalf — an agent tool call (with some confirmation mechanism), or a direct UI button click bypassing the agent?

This is the single most important security boundary in the design. A mutating agent tool means the agent's instruction-following discipline (and prompt-injection resistance) is the boundary. A UI button click means the boundary is "a real user touched a button." This maps to `requirements.md §8` OQ-16.

## Decision

All workflow mutations execute via **direct RPC calls from the UI**, triggered by button clicks on preview components. The chat agent has **zero mutating workflow tools** — only read tools (`workflow_list`, `workflow_get`, `workflows_list_runs`, `workflows_get_run`) and propose-only tools (`workflow_propose_create`, `workflow_propose_update`, `workflow_propose_delete`, `workflow_propose_enable`, `workflow_propose_disable`, `workflow_propose_run_now`). Propose tools return payloads; only a UI button click can commit them.

## Alternatives considered

**Agent-in-the-loop with confirmation tokens.** The propose tool returns a one-time token; a follow-up `workflow_create_from_proposal(token)` tool commits. Rejected because (a) the harness still has to enforce that the token came from a real user-confirmed click, which devolves back into the OQ-14 problem (ADR-010), and (b) every additional tool surface is an attack vector — once `workflow_create_from_proposal` exists, a prompt-injected agent might fabricate or replay tokens.

**HMAC-validated agent tool calls.** The propose tool returns an HMAC-signed payload; the commit tool validates the HMAC. Rejected for the same reason — the validation logic has to verify "this HMAC was generated in response to a real user click," which is exactly what the UI-direct-call pattern already does by construction.

## Consequences

### Positive
- **Structurally impossible for the agent to mutate a workflow.** There is no tool to call. The harness doesn't need to validate confirmation tokens, classify affirmative text, or trust the agent's discipline.
- The mutation surface is the same regardless of who clicked the button — direct UI action and chat-driven preview both hit `workflows_create` exactly the same way.
- A unit test in `agent_tools_tests.rs` can prove the property: `tools::registry::list_tools()` must contain no `workflow_create` / `workflow_update` / `workflow_delete` / `workflow_create_from_proposal`. This is checked in Phase 1 acceptance criteria.

### Negative
- The conversational continuity needs a synthetic-user-message workaround after click — the UI posts *"Saved as `<name>`."* into the chat thread so the agent has context on its next turn (`systemsdesign.md §4.1`, §9.4).
- Three rich preview components must be designed and shipped (Save/Edit/Delete previews) — substantial UI work for what could've been "one tool call."

### Neutral
- Run-Now and Enable/Disable also go through the propose pattern in chat contexts, even though direct UI toggles bypass it (FR-1.3.5). The pattern is uniform.
- A future agent feature (e.g., scheduled-by-agent workflow creation) cannot bypass this boundary without an explicit redesign.

## Implementation notes

- `src/openhuman/workflows/agent_tools.rs` — only read + propose tools. Tests in `agent_tools_tests.rs` assert no mutating tool is registered (Phase 1 DoD).
- `src/openhuman/workflows/rpc.rs` — owns every mutation.
- NFR-2.3.6 codifies this: *"Mutation surface is closed for agents."*
- Frontend: preview component click handlers call `workflowsClient.create()` / `.update()` / `.delete()` directly.

## Related ADRs

- ADR-010 (Button confirmation, not text matching) — the click is the confirmation; ADR-010 defines the UX, this ADR defines the structural security boundary.
- ADR-016 (Sub-agent tool allowlist) — the `agent_prompt` node's sub-agent also has no mutating workflow tools, applying this principle recursively.
