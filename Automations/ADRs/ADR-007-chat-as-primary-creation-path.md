# ADR-007: Chat is the primary workflow-creation path

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

Other automation platforms (n8n, Zapier, Make) lead with a visual canvas. The user drags nodes, configures each one, and connects edges. OpenHuman could follow that pattern, but the platform's unique strength is conversational interaction — the user already talks to the agent for one-off actions, and a workflow is essentially "this thing I just told you to do, but on a schedule."

The question is whether the *hero* creation interaction is a form, a canvas, or chat. This maps to `requirements.md §8` OQ-11.

## Decision

We will make **chat the primary, hero creation path**. The user describes the workflow conversationally; the chat agent calls `workflow_propose_create`; the drafting sub-agent (`proposer.rs`) returns a `WorkflowProposal`; the agent emits it as a `<WorkflowProposalPreview>` rich message component with Save / Discard buttons. A click on Save dispatches the `workflows_create` RPC directly from the UI. A form fallback exists for power users at lowest priority; the visual canvas is deferred to Phase 3.

## Alternatives considered

**Form-first.** Lead with a "Create workflow" form: name, trigger config, one node config, save. Rejected because it's redundant with chat — a user who can describe what they want in a sentence shouldn't have to fill the same description into four form fields. Form is preserved as a low-emphasis fallback (FR-1.3.1.3), not as the hero.

**Visual-canvas-first.** Build the Phase 3 canvas immediately. Rejected because (a) the canvas adds substantial scope to Phase 1 (`@xyflow/react`, node palette, edge wiring, config drawers), and (b) the agent + chat already cover creation *and* editing — the canvas is non-blocking for the product story per `prd.md §5 Phase 3`.

## Consequences

### Positive
- Plays to OpenHuman's strength: the agent already exists, the user already talks to it. Workflows feel like a natural extension of normal chat, not a separate app.
- Drastically smaller Phase 1 surface — no canvas means no node-palette UI, no edge-drag interactions, no per-node config drawers.
- Editing is symmetric to creation ("rename my retweet workflow") — one mental model for both.

### Negative
- The chat path carries the full reliability burden of the drafting sub-agent. If the LLM hallucinates a connection or emits invalid JSON, the user feels it. ADR-015 (bounded auto-retry) and ADR-019 (structured validation errors) exist to manage that risk.
- Discoverability — a user who's never chatted with the agent has to learn that "ask in chat" is the way to create a workflow. The empty-state CTA ("Ask OpenHuman to build a workflow") and the starter catalog cover this.

### Neutral
- The form fallback is intentionally low-emphasis; we expect it to see < 10% of creation traffic and may remove it in a later phase if telemetry confirms.

## Implementation notes

- `src/openhuman/workflows/proposer.rs` — drafting sub-agent + `draft_with_retries`.
- `src/openhuman/agent/prompts/workflow_builder.md` — drafting sub-agent system prompt.
- `app/src/components/workflows/preview/WorkflowProposalPreview.tsx` — the rich message component.
- See `systemsdesign.md §4` for the full flow diagram.

## Related ADRs

- ADR-010 (Button confirmation, not text matching) — defines how chat-driven Save actually commits.
- ADR-012 (UI-direct mutations) — the agent has no mutating tools; chat creates the proposal, the UI commits.
- ADR-015 (Bounded auto-retry on validation failure) — handles drafting-agent reliability.
- ADR-020 (Workflow proposal preview design) — the UX of the preview component itself.
