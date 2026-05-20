# ADR-010: Confirmation via interactive buttons, not text matching

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

In the chat-driven creation path, the agent emits a workflow proposal and the user needs to confirm or reject it. The confirmation mechanism has security implications: if the agent itself can decide that the user confirmed (e.g., by classifying a "yes" message), then the security boundary is the LLM's classification accuracy, not an explicit user action.

The choices are: regex matching of affirmative text replies ("yes"/"ok"/"sure"), an LLM classifier deciding whether the user confirmed, prompt-only enforcement ("only persist if the user says yes"), or an interactive UI component with explicit Save/Discard buttons. This maps to `requirements.md §8` OQ-14.

## Decision

Confirmation is an interactive **`<WorkflowProposalPreview>` rich-message component** with explicit [Save (paused)] / [Save & Enable] / [Discard] buttons. The component carries the full `WorkflowProposal` payload. Clicking a button invokes the corresponding `workflows_*` RPC directly from the UI via `workflowsClient`. There is no text-matching of user replies, no LLM-classifier confirmation, and no harness-side regex on user messages.

## Alternatives considered

**Regex affirmative matching.** The harness scans the user's next message for "yes"/"ok"/"sure" and triggers the mutation if matched. Rejected because (a) it's brittle across languages (`requirements.md §2.7` requires i18n), (b) it conflates a casual conversational "ok" with explicit intent to commit, and (c) the security boundary becomes a regex, which is an unacceptably low bar for a mutating action.

**LLM-classifier confirmation.** A small classifier model decides whether the user's reply expresses confirmation. Rejected because it inherits the LLM's failure modes — a misclassification leads to an unintended workflow being persisted. Even at 99% accuracy, that's a regular source of silent mistakes for a mutating action.

**Prompt-only enforcement.** Tell the drafting agent in its system prompt to only persist after an explicit user confirmation. Rejected because it relies on the agent's instruction-following discipline, which is exactly what we shouldn't trust for security-critical actions. The agent doesn't even need to be malicious — a confused agent or a prompt-injection attack can subvert it.

## Consequences

### Positive
- Confirmation is an explicit, deterministic user action — a button click that the UI directly translates into an RPC. No model in the loop on the commit path.
- Works identically across languages — the button label is translated, but the click semantic is universal.
- Defensible against prompt injection — even a maliciously crafted proposal cannot persist itself because the agent has no tool to call (see ADR-012).

### Negative
- We have to design and implement the rich-message component (`<WorkflowProposalPreview>` and its siblings `<WorkflowEditPreview>`, `<WorkflowDeletePreview>`, `<WorkflowStatePreview>`) and register them with the chat-runtime renderer.
- Edit and delete flows require their own preview components — three rich components total in Phase 1.

### Neutral
- Future preview variants (e.g., a Phase 2 schedule-change preview) extend the same pattern.
- See ADR-020 for the visual design synthesis of the preview component.

## Implementation notes

- `app/src/components/workflows/preview/WorkflowProposalPreview.tsx` — Save/Discard buttons, payload prop.
- Registered with `ChatRuntimeProvider` rich-message renderer (NFR-2.5.7).
- Click handlers invoke `workflowsClient.create()` / `.enable()` directly — no agent in the loop.
- After click and RPC success, the UI posts a synthetic *"Saved as `<name>`."* user message via `chat.append_user_message` for conversational continuity.

## Related ADRs

- ADR-007 (Chat as primary creation path) — defines the surrounding flow this confirmation slots into.
- ADR-012 (UI-direct mutations, no mutating agent tools) — the structural reason no LLM-driven confirmation is possible.
- ADR-020 (Workflow proposal preview design) — the visual design of the component.
