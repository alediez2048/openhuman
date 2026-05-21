# Phase 2 — Execution Expansion

Builds on the Phase 1 foundation. Lands every remaining Phase 1+2
trigger type and node kind, multi-node linear chains with on_error
policy, the inter-node data-passing model, and RU-5..RU-9 starter
templates.

> **Status:** Drafted — not started. Phase 1 ships first; Phase 2
> brainstorm + ticket lock happens after Phase 1 user feedback.
> Per `prd.md §5.2`.

---

## Scope (locked by `prd.md` + `requirements.md`)

The user can:

- **Build chained workflows** ("scan Gmail → score with the agent →
  send the top 3 to Slack") — Phase 2 unlocks multi-node linear
  chains.
- **Trigger via webhook** — the inbound bridge for n8n / Zapier /
  IFTTT / Make and any other webhook source (ADR-005 hybrid
  positioning).
- **Trigger via Composio event** — react to a Composio toolkit's
  webhook ("when Stripe receives a payment...").
- **Trigger via channel message** — Slack / Discord / Telegram
  inbound messages with optional filters.
- **Send messages from workflows** — `channel_message` node ships
  to any connected chat channel.
- **Call any tool** — `tool_call` invokes a single named tool
  from `tools::registry`.
- **Make outbound HTTP** — `http_request` node hits any
  Generic-HTTP connection from Phase 0; templating substitutes
  upstream node outputs into URL / headers / body.
- **Branch** — `condition` evaluates a text-match predicate; the
  executor takes the matching edge.
- **Wait** — `delay` pauses a run with persistent resume across
  core restarts.
- **Recover from per-node failures** — `on_error = Continue`
  lets the run keep going; `on_error = Halt` is the F-7
  default.

---

## The Phase 2 tickets

| Ticket | Title |
|---|---|
| F2-1 | Phase 2 scaffold — `NodeKind` / `NodeConfig` / `Trigger` Phase-2 variants reachable in validator + executor stubs |
| F2-2 | Multi-node execution — linear chain walk + edge ordering + inter-node context |
| F2-3 | `tool_call` node kind |
| F2-4 | `http_request` node kind + templating |
| F2-5 | `channel_message` node kind |
| F2-6 | `condition` node kind |
| F2-7 | `delay` node kind + persistent resume |
| F2-8 | `on_error` policy + per-node retry budget |
| F2-9 | `webhook` trigger via `webhooks::TunnelRegistration::Workflow` |
| F2-10 | `composio_event` trigger subscriber |
| F2-11 | `channel_message` trigger subscriber + filter |
| F2-12 | RU-5..RU-9 starter templates exercising new kinds |
| F2-13 | Workflow-builder prompt update for the Phase 2 taxonomy |
| F2-14 | 30-day soft-delete retention sweep (FR-1.3.4 deferred from Phase 1) |
| F2-15 | `active_hours` enforcement on cron triggers |
| F2-16 | Phase 2 hero E2E (webhook → tool_call → http_request) + catalog E2E + DEVLOG closure + ADR drift audit |

Each ticket follows the Phase 1 F-N format: a primer file with
"What Is This Ticket", "What Was Already Done", "Files to
Create / Modify", "Deliverables Checklist", "Branch & Merge
Workflow", "Architectural Decisions", "Estimated Time", "After
This Ticket".

---

## Pre-Phase-2 brainstorm (do this before starting F2-1)

Five OQs need resolution before F2-1 commits to an architecture:

| OQ | Question | Lean |
|---|---|---|
| OQ-4 | Phase 2 triggers beyond webhook + composio + channel_message? | None — these 3 cover the immediate needs |
| OQ-5 | Run-history retention default? | 30 days (FR-1.3.4 already references this) |
| OQ-7 | Inter-node data passing: literal / templating / expressions? | Templating: `{{node.<id>.output.<jsonpath>}}` |
| OQ-13 | Per-node retry policy shape? | `{ max_attempts: u32, backoff: Exponential { initial_ms, max_ms } }` |
| OQ-14 | Webhook payload available to `agent_prompt` nodes? | Yes — inject as `{{trigger.payload}}` in the prompt context |

Lock each OQ in the brainstorm, drop the resolution into
`requirements.md §8`, then commit F2-1.

---

## Pre-existing dependencies

- **F-1** types declare every Phase 2 `NodeKind` variant already
  (declared from day 1 so adding variants is a code-only change,
  no schema bump per F-1's ADR-018 rationale). F2-1 just needs to
  add the matching `NodeConfig::*` payload variants + plumb them
  through the validator.
- **Phase 0 Connections Hub** ships `GenericHttpConnection` with
  encrypted credentials — F2-4's `http_request` node consumes it.
- **`webhooks/` tunnel domain** already exists with the
  `TunnelRegistration` enum F2-9 extends.
- **Composio `DomainEvent::ComposioTriggerReceived`** event
  already publishes — F2-10 just subscribes.
- **Channel domain** publishes `ChannelMessageReceived` —
  F2-11 subscribes with an optional filter.

---

## Phase 1.5 carried forward

If the Phase 1.5 work (chat-runtime preview rendering + agent
invocation swaps) ships before Phase 2 kickoff, it's the
prerequisite. Otherwise Phase 2 F-13 (prompt update) also has to
account for it.

---

## After Phase 2

- **Phase 3** (visual canvas, deferred) — only revisit if Phase 1
  + Phase 2 ship and users repeatedly ask for it.
- **Phase 4** (deeper external-platform integrations) — n8n
  custom node, Zapier directory app, IFTTT applet, Make custom
  app. Each its own project; only pursue with concrete user
  demand per `prd.md §5.4`.
