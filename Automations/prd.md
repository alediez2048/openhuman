# Workflows & Automations — Product Requirements Document

**Status:** Draft (brainstorming in progress)
**Owner:** Ale Diez (`alediez2408@gmail.com`)
**Target repo:** [tinyhumansai/openhuman](https://github.com/tinyhumansai/openhuman)
**Contribution intent:** Open-source PR series shipping a new "Workflows" surface in OpenHuman.

---

## 1. Problem Statement

OpenHuman gives users a powerful conversational personal agent with deep memory and a growing catalog of connected services (Composio ~1000 services, native channels, webview scrapers for Twitter / LinkedIn / WhatsApp / Slack / Discord, etc.). The agent can take any one-off action via chat.

However, **there is no first-class surface for the user to set up and manage long-running, background automations** — the kind n8n / Zapier / Make are built around. Triggers (cron, webhooks, Composio events) and execution (the agent + connected tools) both exist as separate primitives inside the codebase, but the user cannot:

- Name and save an automation as a reusable, editable entity.
- Compose a trigger with one or more steps in a single configurable unit.
- See a list of "things OpenHuman is doing for me in the background."
- Inspect run history per automation.

This gap is the difference between *"an assistant you ask"* and *"a personal agent that works for you."*

---

## 2. Vision

A dedicated **Workflows** navigation item in OpenHuman where the user can:

- **Ask OpenHuman to build a workflow in chat.** The hero interaction. The user says "every time I retweet, post to LinkedIn in my voice" — the agent drafts the workflow JSON, previews the structure, and on confirmation persists it as paused. This is the daily-use creation path; the visual canvas (Phase 3, deferred) is a power-user fallback.
- **Toggle workflows on/off from the nav.** Activation is one click on each workflow card. The Workflows tab is a monitoring + activation surface first, an editing surface second.
- **Open the app to seeded starter workflows.** On first run, a curated set (drawn from the 9 reference use cases below) lands in the user's workflows list as `paused`. The user activates the ones they want; the rest can be deleted. No empty-state friction.
- **Use the OpenHuman agent as the engine.** Workflows leverage memory, connected accounts, and the existing tool surface — OpenHuman's unique differentiator vs. n8n / Zapier / IFTTT / Make.
- **Run on local infrastructure.** The user's own machine via the existing core process. No SaaS dependency for the engine itself.
- **Interoperate with external automation platforms** (n8n, Zapier, IFTTT, Make) as just-another-connection: inbound triggers via the existing `webhooks/` tunnel, outbound actions via a generic `http_request` node (both Phase 2). Same plumbing covers any REST service on the planet.
- **Manage every connection from one place.** The Phase 0 Connections Hub unifies Composio, Channels, Webview accounts, Built-in integrations, MCP servers, and Generic HTTP into a single surface — the source the agent draws from when building a workflow.

---

## 3. Hero User Story

> *"Every time I retweet something on Twitter, I want OpenHuman to draft a LinkedIn post in my voice with my take on the tweet, and publish it to LinkedIn."*

**Why this story:** it touches the full stack — a webview-account trigger (Twitter), the agent (voice + draft generation against memory), a webview-account action (LinkedIn post), and a background schedule. If this workflow works end-to-end, the architecture is validated.

### 3.0 Hero Interaction (how the user actually builds the above)

The user does **not** open a form or canvas to create the workflow. They open chat, describe what they want, and OpenHuman drafts it as a rich preview component:

```
You: every time I retweet something, draft a LinkedIn post in my voice
     and publish it
OpenHuman: I'll build that as a workflow.

  ┌─────────────────────────────────────────────────────────────────┐
  │ Proposed workflow                                               │
  │ ─────────────────                                               │
  │ Name: Retweet → LinkedIn post                                   │
  │                                                                 │
  │ Trigger: every 15 minutes, check my recent retweets on Twitter  │
  │                                                                 │
  │ Step 1 (agent_prompt): for each new retweet, draft a LinkedIn   │
  │   post in my voice from memory and publish.                     │
  │   Connections: twitter (webview), linkedin (webview), memory    │
  │                                                                 │
  │ [ Save (paused) ]  [ Save & Enable ]  [ Discard ]               │
  └─────────────────────────────────────────────────────────────────┘
```

The user clicks **[Save (paused)]** or **[Save & Enable]**; the UI calls the `workflows_create` RPC directly (the agent has no mutating tools — see "Mutation path" below). The workflow appears in `/workflows`, and the agent sees a synthetic *"Saved as Retweet → LinkedIn post"* message on its next turn.

**Mutation path:** Agent-driven mutations are funneled through this same preview-and-click contract. The agent has *no* mutating tools — only read tools (`workflow_list`, `workflow_get`, ...) and propose-only tools (`workflow_propose_create`, `workflow_propose_update`, `workflow_propose_delete`, `workflow_propose_enable`, `workflow_propose_disable`, `workflow_propose_run_now`). Each propose tool returns a payload that renders as an interactive preview component; the user's click is what triggers the actual RPC. This makes silent agent-driven mutation literally impossible.

**Missing connections:** If the proposal references connections the user doesn't have (e.g., the user hasn't set up Twitter or LinkedIn webview accounts yet), the preview still shows Save — the workflow saves with `health: NeedsConnections { missing }` and the list-view card shows ⚠️ "Needs Twitter, LinkedIn." The toggle stays disabled until the user wires up the connections; once they do, an event-bus subscriber automatically recomputes health and the toggle enables.

**Out-of-catalog requests:** If the user asks for something OpenHuman can't natively trigger ("when my heart rate spikes…"), the agent proposes a workflow with a `Webhook` trigger and a `setup_instructions` paragraph explaining how to wire it up via the generic tunnel URL. The escape hatch is universal — every external source maps to a webhook.

### 3.1 Additional Reference Use Cases

The following nine workflows are touchstones used throughout the systems-design doc, the ticket primers, and the Phase 4 template catalog. Every per-phase DoD must keep them implementable. Together they span every supported trigger type, every connection mechanism (Composio, native channels, webview accounts, integrations, memory), and every node pattern v1 needs to express (linear chain, conditional, fan-out, human-in-the-loop).

Connection vocabulary used below:

- **Composio** — toolkit ids from the curated catalog (`gmail`, `notion`, `slack`, `github`, `discord`, `googlecalendar`, `linear`, `jira`, `spotify`, `stripe`, ...) — see `src/openhuman/composio/providers/mod.rs`.
- **Channel** — a native chat-channel provider (`telegram`, `slack`, `discord`, ...) — see `src/openhuman/channels/`.
- **Webview** — a CEF-hosted account scraper (`linkedin`, `twitter`, `whatsapp`, ...) — see `app/src-tauri/src/webview_accounts/`.
- **Meet** — the existing `meet/` and `meet_agent/` integration.
- **Memory** — the OpenHuman memory tree.

#### RU-1 · Founder morning digest
*Trigger: `cron` (8am weekdays) · Connections: Composio (`gmail`, `linear`, `slack`), Channel (`telegram`), Memory*

> *"Every weekday at 8am, read my unread Gmail, my Linear-assigned issues, and any unread Slack DMs. Using your memory of what I'm working on this week, summarize what actually needs my attention today and send a single message to my Telegram."*

#### RU-2 · LinkedIn engagement queue (human-in-the-loop)
*Trigger: `cron` (11am weekdays) · Connections: Webview (`linkedin`), Channel (`telegram`), Memory*

> *"Every weekday at 11am, scan my LinkedIn feed for the 5 most relevant posts in my industry, draft thoughtful comments in my voice, and queue them in a Telegram chat where I can swipe through and approve before they post."*

Surfaces the need for an `await_human_approval` node kind — useful forcing function for the Phase 3 canvas design.

#### RU-3 · Spotify "Friday Five" → Discord
*Trigger: `cron` (Friday 5pm) · Connections: Composio (`spotify`, `discord`), Memory*

> *"Every Friday at 5pm, look at the songs I added to my Spotify library this week, pick the top 3 by listening frequency, and post a 'Friday Five' to my personal Discord server with a one-line description of each in my voice."*

Pure Composio-to-Composio workflow with the agent providing voice.

#### RU-4 · Biweekly Jira sprint retrospective → Notion
*Trigger: `cron` (every other Friday 4pm) · Connections: Composio (`jira`, `notion`), Memory*

> *"Every other Friday at 4pm, pull all closed Jira tickets from the current sprint, group them by epic, write a retrospective summary in my voice highlighting wins and learnings, and publish it to a new page in my Notion 'Sprint Reviews' database."*

Stresses batch-read patterns and long-form agent synthesis.

#### RU-5 · GitHub stars → Notion tool catalog
*Trigger: `composio_event` (`github.star_added`) · Connections: Composio (`github`, `notion`), Memory*

> *"Whenever I star a GitHub repo, fetch its README, draft a two-sentence summary in my voice explaining what caught my eye and how I might use it, and append a row to my 'Tools to Try' Notion database."*

Tests the composio-event trigger path through `triage` → workflow runner.

#### RU-6 · Calendar interview prep
*Trigger: `composio_event` (`googlecalendar.event_created`) · Connections: Composio (`googlecalendar`, `notion`, `slack`), Memory*

> *"Whenever a new Google Calendar event tagged 'interview' or '1:1' is added, pull the attendees' info from my Notion CRM, create a pre-meeting brief in a new Notion page with talking points based on our last interactions, and DM me a link via Slack so I can review it before the meeting."*

Surfaces the need for a `condition` node (title prefix match) and a multi-tool `agent_prompt`.

#### RU-7 · Stripe failed payment → multi-channel recovery
*Trigger: `composio_event` (`stripe.payment_intent.payment_failed`) · Connections: Composio (`stripe`, `linear`, `slack`, `gmail`)*

> *"When a Stripe payment fails on a customer with more than $500/mo MRR, immediately create a Linear ticket in my Customer Success project, post a redacted alert to the #revenue Slack channel, and draft a recovery email in my Gmail drafts folder."*

Mostly deterministic — validates that workflows compose well without an `agent_prompt` step. Also forces conditional branching on customer MRR.

#### RU-8 · Slack bug triage → Linear
*Trigger: `channel_message` (Slack DM containing "bug:") · Connections: Channel (`slack`), Composio (`linear`)*

> *"When someone DMs me on Slack with the word 'bug:' anywhere in the message, create a Linear ticket in my Inbox project, summarize the issue clearly, label it 'needs-triage', and reply to the Slack thread with the ticket URL."*

Validates the inbound-channel reactive pattern + reply-to-same-channel.

#### RU-9 · Meeting follow-up
*Trigger: Meet integration event (`meet.call_ended` with duration > 15 min) · Connections: Meet, Composio (`gmail`), Memory*

> *"After every Google Meet call that lasts longer than 15 minutes, transcribe the audio, extract action items, and draft follow-up emails to each attendee in my voice — using your memory of past interactions with them. Drop the drafts in my Gmail drafts folder so I can review and send."*

Stresses fan-out semantics (one drafted email per attendee) — the trickiest pattern in v1 and the one most likely to defer to Phase 3.

---

## 4. Target User

A power user of OpenHuman who has:

- Completed onboarding.
- Connected at least one Composio integration OR webview account.
- A clear intent like *"I want OpenHuman to handle X automatically when Y happens."*

Out of scope for v1: enterprise / team-shared workflows, no-code business users who want pre-baked templates only.

---

## 5. Phased Scope

The full vision is too large for one PR. Decomposed into a prerequisite Phase 0 and three core phases. Phase 3 (visual canvas) and Phase 4 (deeper integrations) are deferred.

**Positioning (resolved, OQ-9 = C):** OpenHuman ships its own native workflow engine. External automation platforms (n8n, Zapier, IFTTT, Make) are treated as *just-another-connection* — addressable via the `webhook` trigger (inbound) and the `http_request` node (outbound). No platform-specific code in v1; same generic plumbing works for every REST service on the planet, not only the named four.

**Creation model (resolved with user, Phase 1 hero):** The primary workflow-creation path is **conversational** — user describes the workflow in chat, the agent drafts the JSON, the user confirms. The visual canvas is a deferred power-user surface.

### Phase 0 — Connections Hub (prerequisite)
**Goal:** Every connection mechanism is visible and manageable from a single, unified surface, regardless of whether the user uses Workflows.

**Deliverables:**
- Rename the existing `/skills` route → `/connections`. Update `nav.connections` label accordingly.
- Add sections to the unified Connections page for all 5 native mechanisms + the Generic HTTP escape hatch: **Composio**, **Chat Channels**, **Browser Accounts (webview)**, **Built-in Integrations** (Twilio, Apify, …), **MCP Servers**, **Generic HTTP**.
- Surface the previously-hidden native integrations (Twilio, Apify, Google Places, Parallel, Seltz, Stock Prices) as toggleable cards with scope controls.
- Surface MCP servers (currently buried in settings) as first-class connections.
- Introduce the **Generic HTTP** connection type — user-defined base URL + auth credentials stored in `security/secrets`, surfaced as a reusable target for `http_request` workflow nodes (and for direct agent use).
- Deep-link from each card to the existing per-mechanism setup flow (no need to rewrite Composio's OAuth flow).
- Search + filter chips across all sections.

**Non-goals:** Workflows feature. Phase 0 is independently valuable and shippable as its own PR before Phase 1 begins.

### Phase 1 — Workflows Foundation + agent-driven creation + seeded templates
**Goal:** Workflows exist as named, persisted, browsable, *runnable* entities created primarily through chat. The hero user story (RU-1 retweet→LinkedIn) is built by the agent in a chat conversation, lands in the user's list paused, and runs once the user activates it. First-time users open `/workflows` and see curated seeded templates ready to toggle on.

**Deliverables:**
- New `workflows/` Rust domain (per `src/openhuman/<domain>/` convention) with `Workflow`, `Trigger`, `Node`, `Edge`, `Run`, `RunStep` types.
- SQLite persistence — separate `workflows.db` (OQ-3 = A).
- JSON-RPC surface (`openhuman.workflows_*`).
- New `/workflows` route + dedicated bottom-tab nav item between Connections and Intelligence (OQ-1 = A).
- **List view as the hero surface** — each card shows name, trigger summary, step summary, status badge, last/next run, and a one-click **enabled/paused toggle** as the primary action. Edit/Run-now/Delete in an overflow menu.
- **Trigger types:** `cron` (reusing existing cron scheduler) and `manual` ("Run now").
- **Node kind:** `agent_prompt` with allowed-connections allowlist (drawn from the Phase 0 Connections Hub).
- `workflow_runs` + `workflow_run_steps` tables with run history view.
- Integration with `scheduler_gate` and `triage` / `agent` for orchestrated runs.
- Event-bus events (`WorkflowDefined`, `WorkflowRunStarted`, `WorkflowRunCompleted`, …).
- **Agent-callable workflow tools (read + propose only, no mutations):**
    - `workflow_list`, `workflow_get`, `workflows_list_runs`, `workflows_get_run` — read-only.
    - `workflow_propose_create` / `workflow_propose_update` / `workflow_propose_delete` / `workflow_propose_enable` / `workflow_propose_disable` / `workflow_propose_run_now` — return a `WorkflowProposal` (or `WorkflowEditProposal`) payload that the chat agent emits as a `<WorkflowProposalPreview>` rich component. The component carries the full payload + Save/Discard buttons. Click → UI calls the corresponding `workflows_*` RPC directly. The agent never sees a confirmation token; the agent literally has no mutating tools.
    - The drafting sub-agent (`proposer.rs`) has a bounded retry budget (3 attempts) with `ProposalValidationError` feedback when a draft fails schema/connection/cron validation.
    - The workflow-builder system prompt (`src/openhuman/agent/prompts/workflow_builder.md`) teaches: the propose-then-click contract, the webhook escape hatch for out-of-catalog triggers, the `setup_instructions` field for when connections are missing or external setup is needed.
- **Starter templates catalog (Phase 1)** — templates ship in-repo as JSON, loaded at runtime by `workflows_list_starter_templates`. They appear as **read-only catalog cards** in a "Starter workflows" section on `/workflows`, *not* auto-inserted into the user's workflow table. Each catalog card has [Add to my workflows] and [Add & Enable] buttons; clicking either calls `workflows_create` with the template payload and `origin = Seed { template_id }`, at which point the row migrates from the catalog into the user's "Your workflows" section. Phase 1 ships RU-1 (Founder morning digest), RU-2 (LinkedIn engagement queue), RU-3 (Spotify Friday Five), RU-4 (Jira sprint retro) — every reference workflow that's expressible with `cron`/`manual` triggers + `agent_prompt` node. Phase 2 ships the rest as the node taxonomy expands.
- Read-only detail view for each workflow showing trigger config + ordered node list (no canvas, no drag-edit). Editing is done via chat ("rename my retweet workflow to X").
- E2E spec covering simplified RU-1 created via chat: user asks → workflow appears in list paused → user toggles on → cron fires → run completes.

**Non-goals:** visual canvas (Phase 3, deferred), Phase 2 trigger types and node kinds, template browse-and-import gallery (a richer template surface beyond seed).

### Phase 2 — Execution expansion
**Goal:** All trigger types and core node kinds are supported, including the external-platform bridge via `http_request` + `webhook` trigger. The agent's workflow-builder prompt is upgraded to use the expanded node taxonomy.

**Deliverables:**
- **New triggers:** `webhook` (inbound HTTP via existing `webhooks/` tunnel — this is the inbound bridge from n8n / Zapier / IFTTT / Make), `composio_event` (subscribe to a Composio trigger event), `channel_message` (Slack/Discord/Telegram inbound).
- **New node kinds:** `tool_call` (any registered tool), `http_request` (generic REST against a Generic-HTTP connection from Phase 0 — this is the outbound bridge to n8n / Zapier / IFTTT / Make), `channel_message` (send-to-channel), `condition` (branch on output match), `delay`.
- Per-node retry policies and `on_error` (`halt` / `continue`).
- Linear chains still — branching nodes scaffolded but UI for branching arrives in Phase 3.
- Additional seeded templates that exercise the new node kinds (RU-5 through RU-9).
- Updated agent workflow-builder prompt that knows about the new triggers and node kinds.

**Non-goals:** visual canvas (Phase 3, deferred).

### Phase 3 — Visual canvas (deferred)
**Goal:** Power-user surface for inspecting and tweaking workflows visually. **Not required for the core product story** — the agent + chat already cover creation and editing.

**Deliverables (only if pursued):**
- `@xyflow/react` integration.
- Node palette (sidebar with drag-source nodes).
- Per-node config drawer.
- Edge wiring with branching support (full DAG, not just linear chain).
- Live run highlighting (a node lights up as it executes).
- Fan-out node semantics (one drafted email per attendee per RU-9).

This phase is **deferred** because the agent-driven creation flow in Phase 1 + the chat-based editing make the canvas non-blocking for the product story. Revisit only if users repeatedly ask for it.

### Phase 4 — Deeper external-platform integrations (deferred indefinitely)
**Goal:** Per-platform polish beyond the generic webhook + `http_request` bridge.

**Deliverables (each its own project):** OpenHuman as a published n8n custom node, a Zapier-directory app, an IFTTT applet, a Make custom app. Only pursued if Phases 0–2 land and there is concrete user demand.

---

## 6. Success Metrics

Tracked qualitatively for an open-source contribution; OpenHuman has anonymized analytics (`OPENHUMAN_ANALYTICS_ENABLED`).

- **Adoption:** % of weekly-active users who have created ≥1 workflow.
- **Retention:** % of workflows that have ≥1 successful run in their first 7 days after creation.
- **Reliability:** % of runs that complete without error.
- **PR quality:** the contribution is reviewable in coherent, single-purpose commits per ticket.

---

## 7. Out of Scope (explicit non-goals)

- Multi-tenant / shared workflows.
- Workflow versioning and rollback.
- A general-purpose expression language. v1 passes upstream node outputs through the agent's natural-language prompt context.
- An online template marketplace.
- Mobile (OpenHuman is desktop today).

---

## 8. Open Questions

> Tracked in `requirements.md §8`. Updated as brainstorming continues. **Resolved** items move to a separate sub-list.

**Resolved (locked-in decisions):**

- **OQ-1 — Nav placement:** ✅ A — Dedicated 8th bottom-tab between Connections and Intelligence; full-page workflows UX.
- **OQ-2 — Phase 1 PR scope:** ✅ B+ — Foundation + minimum execution + agent-driven creation + seeded templates.
- **OQ-3 — Storage:** ✅ A — Separate `workflows.db` per existing OpenHuman convention; `connections.db` for Phase 0 generic-HTTP.
- **OQ-9 — Positioning vs n8n/Zapier/IFTTT/Make:** ✅ C — Hybrid: native engine + external platforms as connections via generic webhook + `http_request`.
- **OQ-10 — Connections Hub sequencing:** ✅ B — Ship as Phase 0 sub-project before Phase 1.
- **OQ-11 — Primary creation path:** ✅ Chat — agent-driven via `workflow_propose` + `workflow_create_from_proposal` tools. Visual canvas deferred to Phase 3 (optional).
- **OQ-12 — Seeded templates:** ✅ Phase 1 — curated subset (RU-1..RU-4) ship as `enabled = false` rows on first run.

**Still open (Phase 1 polish):**

- **OQ-6** — Bottom-tab icon (now needed since OQ-1 = A).
- **OQ-8** — Template storage: in-repo JSON via `include_str!` vs. fetched from backend. Lean: in-repo JSON.

**Deferred (resolved during respective phase brainstorms):**

- OQ-4 — Phase 2 trigger types beyond webhook + composio + channel_message.
- OQ-5 — Run-history retention default (30 days proposed).
- OQ-7 — Inter-node data passing (literal / templating / expressions). Phase 2+.
