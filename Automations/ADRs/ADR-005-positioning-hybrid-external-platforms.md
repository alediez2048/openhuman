# ADR-005: Positioning vs n8n/Zapier/IFTTT/Make — hybrid native engine + external-as-connection

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1, 2
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

The automation space is crowded: n8n, Zapier, IFTTT, and Make all solve "trigger + action chain." OpenHuman could compete head-on, interoperate only, or build a native engine that also speaks the existing platforms' protocols. The choice shapes whether Workflows is a *replacement* for those platforms or a *complement*. This maps to `requirements.md §8` OQ-9.

The unique OpenHuman differentiator is the agent: voice, memory, connected accounts. Native execution lets us put that agent inside an automation step in a way no external platform can.

## Decision

We will ship a **hybrid positioning**: OpenHuman has its own native workflow engine *and* treats every external automation platform as just-another-connection. Specifically — n8n, Zapier, IFTTT, Make, and any other webhook-or-REST service are addressed via the `webhook` trigger (inbound, Phase 2) and the `http_request` node (outbound, Phase 2) running against a Generic-HTTP connection from the Phase 0 hub. No platform-specific code in v1; the same generic plumbing covers every REST-speaking service on the planet.

## Alternatives considered

**Compete head-on.** Build a full n8n replacement with hundreds of pre-baked integrations and a power-user canvas. Rejected because OpenHuman doesn't need to win the integration count race — Composio + native channels + webview accounts already give it broader reach for the long tail. The differentiator is the agent, not integration breadth.

**Interoperate only (no native engine).** Defer all execution to external platforms; OpenHuman just emits webhooks for them to consume. Rejected because it strands the agent's value — an `agent_prompt` node that runs inside the user's local OpenHuman context (with memory, with webview accounts) is fundamentally impossible to host inside Zapier. Without the native engine, the hero user story doesn't work.

## Consequences

### Positive
- Phase 2 unlocks compatibility with the entire external-automation ecosystem through one webhook trigger and one HTTP node — no per-platform integrations to maintain.
- The native engine keeps the OpenHuman agent's full power inside a workflow step.
- We don't have to take a stance on "are we better than n8n?" — users can run both, with OpenHuman handling agent-driven steps and external platforms handling whatever they're already configured for.

### Negative
- Two layers to communicate to users — "your own automation engine *plus* a bridge to other ones" is more nuanced than a pure positioning statement.
- No platform-specific polish (e.g., a published n8n custom node) in v1 — Phase 4 sketches that as deferred indefinitely until users ask.

### Neutral
- Phase 4 (per-platform custom apps) remains a possibility but is not part of any committed roadmap. We'll only pursue it if Phase 2 lands and users demand named-platform polish.

## Implementation notes

- Phase 2 `webhook` trigger extends `webhooks::TunnelRegistration` with `Workflow { workflow_id }`. HMAC-verified per NFR-2.2.4.
- Phase 2 `http_request` node config references a Phase 0 `generic_http_connections.id` by soft string id (see ADR-003).
- See `systemsdesign.md §10` for the interop architecture.

## Related ADRs

- ADR-006 (Connections Hub as Phase 0) — Generic HTTP connection type is the substrate for the outbound bridge.
- ADR-013 (Webhook escape hatch) — uses the same Phase 2 webhook trigger as the catch-all for unknown trigger sources.
