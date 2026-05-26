# F4 Initiative Primer — Campaigns + Workflow UX

**For:** New coding-agent session(s) executing Phase 4
**Project:** OpenHuman — Workflows & Automations
**Date drafted:** 2026-05-26 (post-grill)
**Dependencies:** Phase 2 (Execution Depth) MUST be on `main`. Phase 3 (Browser Agent) is NOT a prerequisite — it executes after Phase 4 per the implicit ordering shift.
**Estimated total scope:** 8–10 weeks of focused work across 18 sub-tickets (`F4-1` through `F4-18`).

---

## What Is This Initiative?

Build a **Campaign** abstraction layer above the Phase 2 workflows engine, with three foundational primitives:

1. **Structured entities** via a pluggable `EntityStore` trait (Google Sheets + Attio adapters at MVP).
2. **Campaign object** that owns N related workflows + a shared entity binding + lifecycle + throttle + approval policy.
3. **Workflow detail view** with hybrid form-and-chat editing, modal connection-launching, and pinned-context chat for natural-language updates.

The thesis: **a "campaign" is the unit of business automation, not a workflow.** A campaign is a long-running stateful process operating on a recordset — vendor outreach over 30 days, content calendars, sales pipelines, ads monitoring. Today's workflows can simulate this via Google Sheets state + multiple loosely-related workflows, but the abstraction is leaky and the user has to manage the plumbing in prompts.

### Why It Matters

The product vision (per `STATE.md` + the 2026-05-22 grill on the autonomous-business-agent endpoint) is that a business owner opens OpenHuman, connects their integrations, and runs *real* business processes end-to-end. The three motivating examples from the 2026-05-26 grill:

1. *"1000-row vendor spreadsheet, daily 20 emails+SMS to vendors, Attio CRM, Gmail, Twilio, handle all conversations for a month."*
2. *"Month of content across all my socials, scheduled."*
3. *"Google Ads campaign with keyword list, daily performance report."*

All three are **campaign-shaped**, not workflow-shaped. None of them fit cleanly into the current Phase 2 model.

### What This Initiative Does NOT Replace

- **Phase 2 workflows.** Campaigns OWN workflows. The workflow runtime, executor, trigger surface, and node kinds all stay.
- **Composio.** The Attio adapter uses Composio's Attio integration. Campaigns are additive.
- **The chat drafter.** Campaigns extend the drafter; they don't replace it. New propose tools (`campaign_propose_create`, etc.) live alongside the existing `workflow_propose_*` family.
- **Canvas.** Stays demand-gated. Phase 4's hybrid editor may make Canvas permanently unnecessary; that's a feature, not a regression.

---

## What Was Already Done (prerequisites)

By the time Phase 4 starts, the following must be on `main`:

- **Phase 0** — Connections Hub at `/connections`. ✅ shipped.
- **Phase 1** — Workflows Foundation (F-1 → F-21). ✅ shipped.
- **Phase 2** — Execution Depth (F2-1 → F2-17 + F2-7b + F2-9b + F2-16b). ✅ shipped on 2026-05-26.

### Existing infrastructure Phase 4 builds on

- `src/openhuman/workflows/types.rs::Workflow` — Phase 4 adds a `campaign_id: Option<CampaignId>` field.
- `src/openhuman/workflows/{ops, rpc, schemas}.rs` — Phase 4 adds parallel surfaces under `src/openhuman/campaigns/`.
- `src/openhuman/workflows/proposer.rs` — Phase 4 extends with campaign-aware drafting; the underlying drafting agent + retry loop is reused.
- `src/openhuman/composio/` — Phase 4 uses Composio's Attio + Twilio integrations as adapter backends.
- `src/openhuman/connections/` — Phase 4 surfaces missing-connection prompts via the modal launcher; the connection flow itself is unchanged.
- `app/src/components/workflows/preview/*` — Phase 4 evolves these into the inline-form-editor + chat-panel combo.

### What's notably absent (and Phase 4 must build)

- A generic structured-entity layer (the `EntityStore` trait + adapters).
- A campaign-as-first-class object (`Campaign` type, `campaigns` table, RPC surface).
- An iteration primitive (`for_each` node kind).
- A campaign-level throttle gate (executor enforcement).
- A draft-mode approval queue (UI surface + push notifications).
- A workflow detail view (replaces the card-with-overflow-menu pattern).
- A pinned-context chat affordance (header chip + system-prompt preamble).

---

## Architecture diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│  Phase 4 — Campaigns + Workflow UX                                  │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  UI surface (F4-11..F4-16)                                  │    │
│  │  ┌──────────────┐  ┌──────────────────┐  ┌──────────────┐   │    │
│  │  │ /campaigns   │  │ /campaigns/<id>  │  │ /chat        │   │    │
│  │  │ list view    │  │ detail view      │  │ with pinned  │   │    │
│  │  │              │  │ + inline edit    │  │ workflow     │   │    │
│  │  │              │  │ + connection     │  │ context      │   │    │
│  │  │              │  │   modal          │  │              │   │    │
│  │  │              │  │ + approval queue │  │              │   │    │
│  │  └──────────────┘  └──────────────────┘  └──────────────┘   │    │
│  └─────────────────────────────────────────────────────────────┘    │
│           │                    │                     │              │
│           ▼                    ▼                     ▼              │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  RPC surface (F4-3)                                         │    │
│  │  campaigns_{list,get,create,update,pause,resume,archive}    │    │
│  │  campaign_propose_{create,update,pause,resume}              │    │
│  └─────────────────────────────────────────────────────────────┘    │
│           │                                                         │
│           ▼                                                         │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Campaign domain (F4-1, F4-2)                               │    │
│  │  Campaign type + lifecycle + store + migration 006          │    │
│  │  workflows.campaign_id FK                                   │    │
│  └─────────────────────────────────────────────────────────────┘    │
│           │                                                         │
│           ├─────────────────┬─────────────────┬────────────────┐    │
│           ▼                 ▼                 ▼                ▼    │
│  ┌─────────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────┐ │
│  │ EntityStore     │ │ for_each     │ │ Throttle     │ │ Approval │ │
│  │ trait (F4-4)    │ │ node (F4-7)  │ │ gate (F4-8)  │ │ queue    │ │
│  │ + Sheets (F4-5) │ │              │ │              │ │ (F4-9)   │ │
│  │ + Attio  (F4-6) │ │              │ │              │ │          │ │
│  └─────────────────┘ └──────────────┘ └──────────────┘ └──────────┘ │
│           │                                                         │
│           ▼                                                         │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Phase 2 workflows engine (unchanged)                       │    │
│  │  executor / scheduler / triggers / node kinds               │    │
│  │  + campaign_id awareness                                    │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Locked decisions (from 2026-05-26 grill)

| # | Decision | Choice |
|---|---|---|
| 1 | Conversation handling | (B) Draft mode first |
| 2 | Architecture shape | β — Campaigns as first-class entity |
| 3 | Entity store | Option 3 — pluggable trait, Sheets + Attio at MVP |
| 4 | Creation surface | Option Y — chat-primary, Canvas deferred |
| 5 | Editor type | (D) Hybrid — form for trivial, chat for non-trivial |
| 6 | Connection-add UX | (i) Modal launch |
| 7 | Chat-panel for updates | (y) Pinned workflow context in global chat |
| 8 | Phase scope | Bundled as Phase 4 (supersedes Canvas) |

These are NOT to be re-litigated mid-execution. If a sub-ticket discovers a fundamental incompatibility, escalate to a new grill before changing scope.

---

## Sub-tickets

See individual primers: `F4-1.md` through `F4-18.md`. Each ticket follows the F2-* primer format.

| Ticket | Title | Scope |
|---|---|---|
| F4-1 | Campaign type + lifecycle | New types in `src/openhuman/campaigns/types.rs` |
| F4-2 | Campaigns table + migration 006 | New SQLite table + workflows FK |
| F4-3 | Campaign CRUD ops + RPC + agent propose tools | Parallel to workflows ops |
| F4-4 | `EntityStore` trait + adapter registry | The pluggable foundation |
| F4-5 | Google Sheets adapter | Universal floor |
| F4-6 | Attio adapter | Demographic ask |
| F4-7 | `for_each` iteration node kind | Executor body + validator + schema |
| F4-8 | Campaign throttle primitive | Executor-side rate limiter |
| F4-9 | Approval queue (draft mode) | UI + RPC + push notifications |
| F4-10 | Campaign-aware drafter prompt | Drafter recognises campaign intent |
| F4-11 | `/campaigns` list route | Replaces `/workflows` as primary |
| F4-12 | Campaign detail view | Structured rendering + chat sidebar |
| F4-13 | Inline form editor | Trivial-field direct edits |
| F4-14 | Per-node-kind form editors | One per Phase 2 node kind |
| F4-15 | Connection modal launcher | Modal `/connections` flow |
| F4-16 | Pinned workflow context in global chat | Header chip + preamble |
| F4-17 | Catalog: RU-10..RU-12 starter campaigns | Vendor outreach, content, ads |
| F4-18 | Hero E2E + Phase 4 closure | Comprehensive Appium + DEVLOG |

---

## Phase 4 DoD

- [ ] All 18 tickets merged on `main`.
- [ ] Hero E2E passes on macOS + Linux CI.
- [ ] Vendor-outreach reference campaign runs end-to-end via chat creation.
- [ ] `about_app::list_capabilities()` returns the Phase 4 entries.
- [ ] `STATE.md` updated to "🟢 Phase 4 shipped".
- [ ] PR opened against `tinyhumansai:main` for upstream review.
