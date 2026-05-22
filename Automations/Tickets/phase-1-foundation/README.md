# Phase 1 — Workflows Foundation

Phase 1 of the Workflows & Automations initiative. Lands the
`/workflows` tab, the chat-driven creation pipeline, the executor
+ scheduler runtime, and the four bundled starter templates.

> **Status:** Shipped 2026-05-21. All 15 tickets merged to
> `alediez2048/main` (the user's fork). The upstream rollup PR
> against `tinyhumansai/openhuman:main` opens once the Phase 1
> bring-up live test passes on `alediez2048/main`.

---

## Scope

The user can:

- **See every workflow** at a glance on the new `/workflows` tab
  (F-4). Health badge stays honest as connections come and go
  (F-3).
- **Add a starter workflow** from the four bundled templates with
  one click ([Add] / [Add & Enable]); catalog auto-dedupes
  against already-added templates (F-5 + F-6).
- **Build a workflow in chat** ("every weekday at 8am, summarise
  Gmail and send to Slack"): the drafting sub-agent emits a
  validated `WorkflowProposal`, renders as a
  `<WorkflowProposalPreview>` card, click [Save & Enable] →
  visible in `/workflows`. (Components ship in F-14; the
  chat-protocol wiring that makes them visible inside agent
  messages is documented as the Phase 1.5 follow-up at the
  bottom of DEVLOG.md.)
- **Run a workflow on the schedule** (cron — F-7) or **manually
  on demand** (`workflows_run_now` — F-7) with the real agent
  invocation through `Agent::from_config().run_single()` (F-15).
- **Cancel a running workflow** (soft cancel — F-9). Current node
  completes; subsequent nodes are skipped; status becomes
  `Cancelled`.
- **Trust the security boundary**: every mutation goes through a
  UI button click — there are zero mutating tools on the agent
  surface (F-10 + F-12 enforcement tests).

The four bundled starter templates (`Automations/Templates/`):

| ID | Name |
|---|---|
| `ru-1-founder-morning-digest` | Founder morning digest (weekday 8am) |
| `ru-2-end-of-week-review` | End-of-week review (Friday 4pm) |
| `ru-3-deal-flow-radar` | Deal-flow radar (every 2 hours) |
| `ru-4-eod-cleanup` | End-of-day cleanup (weekday 6pm) |

---

## The 15 tickets

| Ticket | Commit | Subject |
|---|---|---|
| F-1 | `62214363` | scaffold workflows/ Rust domain |
| F-2 | `947474f5` | CRUD RPCs + WorkflowOrigin discriminator wiring |
| F-3 | `42c8f9e6` | WorkflowHealth recompute + ConnectionAdded subscriber |
| F-4 | `ca9b27cd` | `/workflows` route + bottom-tab + list view + empty state |
| F-5 | `5a4458f1` | RU-1..RU-4 starter templates + `list_starter_templates` RPC |
| F-6 | `3f0e37f1` | `StarterWorkflowsSection` + [Add] / [Add & Enable] catalog UI |
| F-7 | `7e66529a` | cron + manual trigger dispatch + `workflows_run_now` / `cancel_run` RPCs |
| F-8 | `5c080475` | `agent_prompt` executor + run history pipeline |
| F-9 | `a5b8a905` | single-flight + soft-cancel + orphan-recovery |
| F-10 | `2fa5ae37` | read-only agent tools + allowlist enforcement |
| F-11 | `55a03fd8` | drafting sub-agent + validator + `draft_with_retries` |
| F-12 | `c18f952d` | propose-only agent tools + `workflow_diff` |
| F-13 | `8acf266b` | lock `workflow_builder.md` as canonical + smoke tests |
| F-14 | `8f8a2d91` | `WorkflowProposalPreview` + companion components |
| F-15 | `152e6717` | hero + catalog E2E + Phase 1 capability + DEVLOG closure |

### Phase 1.5 polish (post-F-15)

The "Phase 1.5" deferred items from F-15 were ALL landed in the same
session. The hero E2E loop works end-to-end today for Composio-routed
workflows:

| Commit | Subject |
|---|---|
| `ca7accba` | wire overflow menu Run / Edit / Delete actions |
| `7a10562c` | persistent "Build a workflow" CTA + Show starter toggle |
| `e6ae9ecc` | label Delete as "Move to starter workflows" for Seed-origin rows |
| `f0a2288c` | wildcard match for empty account_id/channel_id in `is_connected` + boot-time recompute sweep |
| `eea486f5` | real agent invocation in drafters + chat-runtime `<workflow-preview>` tag rendering |
| `90e4b7d6` | draft Phase 2 + Phase 3 ticket sets |
| `23645a25` | teach chat agent about the Workflows feature (orchestrator prompt) |
| `4c54e649` | expose workflow tools in the orchestrator `[tools].named` allowlist |
| `b0e3b73c` | register `channel_send` + `webview_account_send` stub tools |
| `1445afb5` | refresh proposer module doc — placeholder body is gone |

---

## E2E surfaces

- **Catalog flow** (`workflows-seeded.spec.ts`, F-15) — NFR-2.6.4.
  Open `/workflows` → 4 starter cards → click [Add] on RU-1 →
  catalog dedupes → your-workflows shows the seeded row →
  delete via direct RPC → catalog regrows.
- **Hero flow** — NFR-2.6.3. Documented as a Phase 1.5
  deliverable: requires the chat-runtime protocol extension that
  makes `<WorkflowProposalPreview>` visible inside
  `AgentMessageBubble`, the chat-agent system-prompt update
  teaching it about `workflow_propose_*` tools, and the F-11 /
  F-12 drafter agent invocation swaps with `emit_proposal` as a
  registered tool. The components and surfaces all exist; this
  is the integration ticket.

---

## DEVLOG

Per-ticket logs live in [`DEVLOG.md`](./DEVLOG.md), one entry per
F-N with files, deviations, and verification commands. The
closure section at the bottom of DEVLOG.md walks every ADR and
notes drift between design intent and as-shipped code.

---

## Deferred follow-ups

- Hero E2E spec file (NFR-2.6.3) — the loop works end-to-end
  manually but no dedicated WDIO spec lives in `app/test/e2e/specs/`
  yet. Tracked in Phase 2.
- 30-day soft-delete retention sweep (FR-1.3.4) — TODO from F-2.
- `active_hours` enforcement on cron triggers — TODO from F-7.
- Dedicated run-history detail view UI — backend (`workflows_get_run`)
  wired today; UI deferred.
- Real Channel + Webview outbound send — Phase 2 (F2-5).
  Sender nodes today resolve to stub tools that return a clear
  deferred-feature error.
- Multi-node chains — Phase 1 ships single-`agent_prompt`-node
  workflows. Phase 2 adds `tool_call`, `http_request`,
  `channel_message`, `condition`, `delay` node kinds.
- Visual-canvas surface — Phase 3, per ADR-002.
- New triggers (`webhook` / `composio_event` / `channel_message`),
  RU-5..RU-9 templates — Phase 2.
