# Phase 4 — Campaigns + Workflow UX

> **Status:** Drafted 2026-05-26. Not started. Locked architecture decisions from the 2026-05-26 grill.
> **Supersedes:** Phase 4 Canvas (`phase-4-canvas/`) — demand never materialised, replaced by Campaigns as the actual Phase 4 deliverable.

---

## TL;DR

Phase 2 shipped multi-node workflows with rich triggers and node kinds. The current architecture is **stateless triggered chains** — fire once, run, done. That's enough for "summarise my morning email" but breaks down for **long-running, stateful campaigns** operating on a recordset (vendor outreach over 30 days, 30-piece content calendars, ongoing ads campaigns).

Phase 4 introduces the **Campaign** abstraction as a first-class entity that owns N related workflows + a structured entity binding (Google Sheets, Attio, eventually native). It also overhauls the workflow detail UX — inline form editing for trivial fields, pinned-context chat for non-trivial edits, modal connection-launching, an approval queue for the draft-mode conversation handler.

The thesis: **a "campaign" is the unit of business automation, not a "workflow."** Users think in campaigns ("the vendor outreach campaign"); workflows are implementation detail underneath.

---

## Scope (locked decisions from the 2026-05-26 grill)

| Decision | Choice | Why |
|---|---|---|
| Conversation handling mode | **(B) Draft mode** first — agent drafts, user approves. (A) auto-reply and (C) triage land later as separate primitives; (D) tiered-per-message policy is the eventual composition. | (B) is the minimum that proves the platform can handle conversations at all. (A) without (B) is reckless; (C) without (B) is just a fancier inbox. |
| Architecture shape | **β — Campaigns as a distinct entity owning N workflows.** New `campaigns` table; `workflows.campaign_id` FK. | Browse + status + throttle + approval policy are campaign-level concerns. α (campaign-as-label) forces all of these to be retrofitted onto workflow. |
| Entity store | **Option 3 — pluggable `EntityStore` trait. Ship Sheets + Attio adapters in parallel from day one.** Future adapters (Airtable, Notion, native `entities.db`) plug in cheaply. | Sheets is the universal floor; Attio is the demographic ask. Locking to one adapter forces refactor when the other lands. |
| Creation surface | **Option Y — chat-primary; Canvas explicitly deferred.** | Differentiator vs n8n is AI-native creation. Three concrete examples (vendor outreach, content campaign, Google Ads) are not graph-shaped — Canvas adds zero value for 3-node chains. |
| Workflow detail editor | **(D) Hybrid — form for trivial fields, chat for non-trivial.** Split rule: *does this edit need a draft preview before commit?* | Right tool for the right job. Form edits commit via direct RPC. Reasoning edits go through `workflow_propose_update`. |
| Connection-add UX | **(i) Modal launch.** Workflow detail detects missing connection, modal opens `/connections` flow without leaving the page. | (ii) embeds connection cards inline (bloats the view); (iii) deep-link punts away (current pattern, not enough). |
| Chat-panel for updates | **(y) Pinned workflow context in global chat.** Workflow detail has "Discuss this" button → opens main `/chat` with workflow pinned. | (x) workflow-scoped local chat would double the chat infrastructure. (y) reuses the existing agent + history. |

Out of scope for Phase 4:

- **Auto-reply mode (A) and triage mode (C).** Land in a later phase once draft mode is battle-tested.
- **Native `entities.db`.** Phase 5 placeholder still applies — adapter pattern in Phase 4 means it's a drop-in addition.
- **Canvas visual editor.** Stays demand-gated per `prd.md §5.3`. The detail view in Phase 4 may make Canvas permanently unnecessary.
- **Tiered policy (D).** Composition of A/B/C; comes after all three exist.
- **Multi-tenancy / campaign sharing.** Single-user for now.

---

## What this unlocks (concrete examples from the grill)

**Vendor outreach (the user's primary example).**
> *"I have a 1000-row spreadsheet of local vendors. I want to send a custom email and text message to each about services I want to provide to them. Use Attio as our CRM, the connected gmail account for email outreach and the connected twilio account for text messages. Run this campaign daily reaching out to 20 businesses every day for the next month, and handle all conversations."*

Maps to: one Campaign + Attio entity binding (Vendor) + Sheets-imported initial set + three sub-workflows (outbound daily batch via Gmail + Twilio; inbound reply handler via composio_event GMAIL_NEW_MESSAGE + channel_message Twilio; daily progress digest). Throttle: 20/day. Conversation policy: draft mode (mode B).

**Content distribution.**
> *"Create a month worth of content for my business, schedule it and send it across all my socials."*

Maps to: one Campaign + ContentPiece entity (status: drafted→approved→scheduled→posted) + sub-workflows (generate batch via agent_prompt; schedule via cron with throttle 1/day; post via channel_message per social; engagement-tracker via composio_event).

**Google Ads.**
> *"Kickstart a google ads campaign, here are the keywords, send me a report daily with performance updates."*

Maps to: one Campaign + AdCampaign entity (via Composio Google Ads adapter as the entity binding) + sub-workflows (setup once via http_request to Google Ads API; daily monitor via cron + http_request; daily report via cron + channel_message).

---

## Phase 4 sub-tickets (F4-1 through F4-18)

Each ticket has its own primer in this directory. Estimated total: **8–10 weeks of focused work.**

### Foundation — Campaigns table + types + CRUD (3 tickets)
- **F4-1** — `Campaign` type + lifecycle (draft / active / paused / wound-down / archived) + supporting types (`Throttle`, `ApprovalPolicy`, `EntityRef`, `CampaignStatus`)
- **F4-2** — `campaigns` SQLite table + migration 006 + `workflows.campaign_id` FK + store CRUD
- **F4-3** — Campaign CRUD ops + RPC surface + agent propose tools (`campaign_propose_create`, `_update`, `_pause`, `_resume`, `_archive`)

### Entity adapters (3 tickets)
- **F4-4** — `EntityStore` trait + `EntityRef` enum + adapter registry + schema discovery surface
- **F4-5** — Google Sheets adapter (read rows, write status column, schema-from-headers)
- **F4-6** — Attio adapter (typed People / Companies / Deals, native record identity, webhook subscriptions)

### Execution primitives (3 tickets)
- **F4-7** — `for_each` iteration node kind: takes an entity query, runs the chain once per matching record
- **F4-8** — Campaign throttle primitive: campaign-level rate-limit ("20/day") enforced by the executor across all sub-workflows
- **F4-9** — Approval queue: draft mode conversation handler stores drafts pending user review; new `/approvals` route + RPC surface + push-notification on new draft

### Drafter integration (1 ticket)
- **F4-10** — Campaign-aware drafter prompt rewrite: recognises campaign-shaped intent, negotiates entity schemas mid-chat, proposes multi-workflow campaigns

### UI surface (6 tickets)
- **F4-11** — `/campaigns` list route + bottom-tab + campaign card component (replaces `/workflows` as the primary user surface; `/workflows` becomes admin/debug)
- **F4-12** — Campaign detail view: structured rendering of campaign + child workflows + entity binding + progress metrics
- **F4-13** — Inline form editor for trivial fields (D-hybrid form mode) — name, description, schedule, connection bindings, etc.
- **F4-14** — Per-node-kind form editors: one structured editor per Phase 2 node kind (agent_prompt, tool_call, http_request, channel_message, condition, delay)
- **F4-15** — Connection modal launcher: in-page modal to launch `/connections` flow when a campaign needs a missing adapter
- **F4-16** — Pinned workflow context in global chat: header chip + system-prompt preamble + `/chat?workflow=<id>` deep-link

### Closure (2 tickets)
- **F4-17** — Catalog: 3 new starter campaign templates (RU-10 vendor outreach, RU-11 content calendar, RU-12 ads monitoring)
- **F4-18** — Hero E2E + Phase 4 closure: comprehensive Appium-driven test of the chat→campaign→entity-bind→approve→execute flow + DEVLOG + ADR drift audit + capability entries

---

## Cross-phase impact

**Phase 3 (Browser Agent)** — moves AFTER Phase 4 per the implicit user-priority shift in the 2026-05-26 grill. The browser agent slots into Phase 4 campaigns naturally as one more node kind once Campaigns ship.

**Phase 5 (Business Entities + Outcome Observability)** — scope reduces. Phase 4's adapter pattern delivers structured entities. Phase 5 becomes: native `entities.db` adapter for users without an external CRM, observability surface for outcome tracking (e.g., "this campaign drove 12 demo bookings"), and the entity-graph cross-campaign queries.

**Phase 6 (Proactive Agent)** — unchanged. Builds on Phase 5's observability. Phase 4 is a prerequisite (proactive proposals need campaign-shaped output).

**Canvas (deferred)** — `phase-4-canvas/` directory stays for historical reference but is no longer numbered Phase 4. May never ship per the original demand-gating rationale.

---

## Dependencies + prerequisites

- ✅ Phase 0 (Connections Hub) — shipped, provides the connection inventory campaigns bind to.
- ✅ Phase 1 (Workflows Foundation) — shipped, provides the underlying workflow row + drafter + executor.
- ✅ Phase 2 (Execution Depth) — shipped, provides the multi-node + trigger surface campaigns wrap.
- 🔵 F2-17b — deferred trigger-bus live-transport scenarios. Worth landing before F4-18's hero E2E.
- 🔵 Phase 1 hero E2E (NFR-2.6.3 deferral) — would be nice but not blocking.

---

## After Phase 4

- **Phase 3 (Browser Agent)** — drafted, executes after Phase 4.
- **Phase 5 (Business Entities)** — placeholder, scope reduced.
- **Phase 6 (Proactive Agent)** — placeholder.

Phase 4 is the major user-facing release. Marketing pitch: *"OpenHuman runs your business processes end-to-end. Describe a campaign in chat, connect your CRM, let it work."*
