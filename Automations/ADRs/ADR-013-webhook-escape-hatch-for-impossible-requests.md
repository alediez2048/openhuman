# ADR-013: Webhook escape hatch for out-of-catalog trigger requests

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1, 2
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

Users will ask for workflows whose trigger source isn't in OpenHuman's catalog — *"when my heart rate spikes…", "when the doorbell rings…", "when my CRM gets a new lead from a custom system…"* etc. The drafting sub-agent has three plausible responses: refuse with an explanation, ask a clarifying question, or propose something that *can* work.

Refusing leaves the user stuck. Asking forces a back-and-forth that may not converge. Proposing something workable requires a universal fallback. This maps to `requirements.md §8` OQ-17.

## Decision

When the user describes a trigger source not in the catalog, the drafting sub-agent proposes a workflow with the **`Webhook` trigger** (Phase 2; Phase 1 falls back to `Manual` with `setup_instructions`) and populates the `setup_instructions` field with a paragraph explaining how to wire up an external service (phone shortcut, IFTTT applet, custom integration) to POST to the generated tunnel URL. The escape hatch is *universal* — every external source on the planet maps to a webhook.

## Alternatives considered

**Refuse with explanation.** *"OpenHuman doesn't support heart-rate triggers. Try one of: cron, webhook, …"* Rejected because it strands the user — they have to figure out the next step on their own without a concrete proposal.

**Propose with `Manual` trigger silently.** Default to a `Manual` trigger that the user has to Run-Now each time. Rejected because it silently downgrades the user's intent ("every time…") into "press a button each time" without telling them.

**Ask clarifying question.** *"How does that event get into a computer system?"* Rejected as the default because it forces conversational ping-pong; the agent should produce a usable proposal in one shot when possible. Clarification remains a fallback if even the webhook framing doesn't fit.

## Consequences

### Positive
- The agent always has a workable answer for unknown triggers — there's no class of request that produces a dead-end response.
- The webhook escape hatch is the same mechanism used for n8n / Zapier / IFTTT / Make interop (ADR-005) — one trigger, two use cases.
- `setup_instructions` is a documented part of `WorkflowProposal` (`systemsdesign.md §2.3`); the preview renders it as a callout above the buttons.

### Negative
- Phase 1 has no `Webhook` trigger yet — Phase 1 falls back to `Manual` with `setup_instructions` describing what would happen post-Phase-2. This is a temporary degradation; the workflow-builder prompt must teach the sub-agent about it (NFR-2.5.6).
- The user is responsible for the actual wiring (e.g., setting up the IFTTT applet that POSTs to the tunnel URL). OpenHuman cannot validate the external side; if the user misconfigures it, the workflow simply never fires.

### Neutral
- Webhook triggers don't depend on connections; `health` is `Ready` from creation, the toggle is enabled, and the workflow waits passively for posts.

## Implementation notes

- Webhook trigger ships in Phase 2 via `TunnelRegistration::Workflow { workflow_id }` per `requirements.md §1.4.2.1`.
- `setup_instructions` field on `WorkflowProposal` populated by the drafting sub-agent.
- The workflow-builder system prompt (`src/openhuman/agent/prompts/workflow_builder.md`) teaches this pattern explicitly with a worked example (`systemsdesign.md §4.2`).
- Preview renders `setup_instructions` as a callout — see ADR-020's design.

## Related ADRs

- ADR-005 (Hybrid positioning) — webhook trigger is the same mechanism used for external-platform interop.
- ADR-007 (Chat as primary creation path) — the escape hatch is what makes chat creation universal.
