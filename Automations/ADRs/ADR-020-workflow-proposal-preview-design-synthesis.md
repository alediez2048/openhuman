# ADR-020: `<WorkflowProposalPreview>` design synthesis — minimalist + inspectable + conversational

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md) · [`Artifacts/designs/workflow-proposal-preview.md`](../Artifacts/designs/workflow-proposal-preview.md)

## Context

The chat-driven creation flow (ADR-007) culminates in the user seeing a rich preview component (ADR-010) and clicking Save or Discard. The component is the single most-seen UI surface for Workflows in Phase 1 — every chat-created workflow passes through it.

Four parallel sub-agent designs explored the design space: Minimalist (140px-tall, three buttons, fast scan), Inspectable (full details panel, scrollable rationale + raw JSON), Conversational (the card morphs into an agent message after Save), and Diff-card (per-field diff rendering optimized for edits). Each had strengths the others lacked.

## Decision

`<WorkflowProposalPreview>` synthesizes from all four designs into a single component with **minimalist base render, inspectable on disclosure, conversational on save**. Specifically:

1. **Base render (~140px tall):** name, one-line description, trigger summary, connection chips, three action buttons (Discard / Save (paused) / Save & Enable).
2. **"Show details" disclosure** expands a sectioned panel below the card with Rationale / Agent prompt / Required connections / Settings — collapsed by default within the panel. No raw-JSON section (developer-tool noise).
3. **Saved-state morph:** after successful Save, the card animates into a one-line *"✓ Saved as `<name>`"* stub *and* a new agent message bubble renders below — the save reads as the agent continuing the conversation, not as a form completing.

Companion components reuse the pattern with variants: `<WorkflowEditPreview>` uses diff-row rendering (from the Diff-card design exploration); `<WorkflowDeletePreview>` is a simple coral-bordered confirmation card; `<WorkflowStatePreview>` is a tighter version with a single primary button for toggle / run-now.

## Alternatives considered

**Pure Minimalist (Designer A).** Ship just the 140px card with three buttons; no disclosure, no details panel. Rejected because users with `confidence: low` proposals need to inspect the rationale before clicking Save — a one-line description doesn't carry enough information for a confident commit.

**Pure Inspectable (Designer B).** Ship the full details panel always-expanded with rationale, prompt, connections, raw JSON. Rejected because (a) it dominates the chat thread visually, pushing previous context off-screen, and (b) most proposals (high confidence, simple triggers) don't need the detail — the disclosure pattern keeps the card scannable by default.

**Pure Conversational (Designer C).** Render every proposal as a sequence of agent message bubbles ("I'll set the trigger to…", "I'll add a step that…"), each with an inline confirm. Rejected because it loses the "atomic commit" semantic — the user wants to evaluate the whole proposal at once, not approve trigger and steps separately.

**Pure Diff-card (Designer D).** Use the per-field diff rendering for *all* previews including create. Rejected because there's no "before" state for a create — diff-card semantics only make sense for edits. It's used as the basis for `<WorkflowEditPreview>` specifically.

## Consequences

### Positive
- Single visual language across create / edit / delete / toggle previews, easing user mental model.
- Minimalist default keeps the chat thread scannable; disclosure progressive discloses complexity.
- Conversational morph preserves the chat-feel after Save — the workflow lands without abruptly switching surfaces.
- Each variant component lives in `app/src/components/workflows/preview/` and is Vitest-testable (NFR-2.6.5).

### Negative
- Four preview components to build, test, and i18n (`<WorkflowProposalPreview>`, `<WorkflowEditPreview>`, `<WorkflowDeletePreview>`, `<WorkflowStatePreview>`) — substantial UI surface for Phase 1.
- The saved-state morph animation requires coordination with the chat-runtime renderer (NFR-2.5.7) — the synthetic *"Saved as `<name>`."* message must appear in the right order.

### Neutral
- Future preview variants (e.g., a Phase 2 schedule-change preview) extend the same pattern.
- Visual tokens (ocean primary, sage / amber / coral for confidence) come from `app/tailwind.config.js` per `CLAUDE.md §Design`.

## Implementation notes

- `app/src/components/workflows/preview/WorkflowProposalPreview.tsx` — minimalist base + Show-details disclosure + saved-state morph.
- `WorkflowEditPreview.tsx` — diff-row rendering per Designer-D shape.
- `WorkflowDeletePreview.tsx`, `WorkflowStatePreview.tsx` — tighter variants.
- Registered with chat-runtime renderer per NFR-2.5.7.
- Detailed visual spec in [`Artifacts/designs/workflow-proposal-preview.md`](../Artifacts/designs/workflow-proposal-preview.md).
- Vitest tests assert click handlers invoke the correct RPC clients (NFR-2.6.5).

## Related ADRs

- ADR-010 (Button confirmation, not text matching) — defines that the component carries the buttons; this ADR defines what they look like.
- ADR-007 (Chat as primary creation path) — defines the flow this component closes.
- ADR-012 (UI-direct mutations) — the click handlers call RPCs directly.
- ADR-013 (Webhook escape hatch) — `setup_instructions` renders as an amber callout in this component.
