# Phase 2.5 — Trust UX

**Bridge phase between Phase 2 (Execution Depth) and Phase 4 (Campaigns).**

Status: Drafted 2026-06-07 after the 2026-06-05 / 06-07 morning-email-digest debugging sessions exposed a structural gap.

---

## TL;DR

Phase 1 + 2 made workflows **work**. F-16 + F-21 (landed 2026-06-05 / 06-07) made workflow **status honest** at the data layer — no more fake green checkmarks. But the **user-facing trust experience** is still missing: when a workflow ends, the user has no way to tell *what actually happened* without grepping the SQLite DB. The structural fix isn't another executor patch — it's the missing **outcome visibility + pre-deploy validation** layer.

Phase 2.5 is **4 tickets, ~1–2 weeks of focused work**. It must ship before Phase 4 because Campaigns ship N workflows per campaign; without Trust UX, every campaign multiplies the "did it really work?" anxiety by N.

---

## Why this phase exists (the debugging evidence)

Concrete failure modes from the 2026-06-05/07 sessions, all caught only because the user noticed the *absence* of an outcome and came to a coding agent:

| Date | Workflow | What the run history showed | What actually happened |
|---|---|---|---|
| 2026-05-30 .. 06-02 | Morning email digest to Slack | Mixed `Succeeded` / `Failed` | Multiple silent fakes: agent produced narrative text but emitted zero `SLACK_SEND_MESSAGE` calls; some runs were narration loops about `text` vs `markdown_text` parameter; the run showed Succeeded |
| 2026-06-05 17:10 | Morning email digest to Slack | `Failed: 401 Unauthorized` | Anthropic API key invalid — caught honestly, but the user had to query the DB to find this |
| 2026-06-07 08:00 (cron) | Morning email digest to Slack | `Succeeded` | Agent narrated *"Let me try opening a DM by sending a message directly to the user's ID as the channel"* ~10x; never emitted any tool_use block; no Slack message sent; F-21 had not landed yet so the run was marked Succeeded |
| 2026-06-07 13:41 | Morning email brief | `Failed: 400 Bad Request` | `claude-opus-4-7` model pin landed in user config; backend rejected unknown tier; caught in 3s by F-16 but the user only knew via DB query |
| 2026-06-07 13:46 | Morning email brief | `Succeeded` (genuinely!) | Agent read calendar + Gmail, sent a clean email brief to alediez2408@gmail.com — but the user thought it failed because the UI showed "Succeeded — [agent narrative]" with no concrete delivery receipt, so they had to check Gmail manually |

**The pattern:** in every case, the user's question was *"did the thing actually happen?"* and the system's answer was *"go look at the DB and Gmail."* This is the structural gap Phase 2.5 closes.

---

## What success looks like

After Phase 2.5 ships, the following user journey works **without coming to a coding agent**:

1. User describes a new workflow in chat
2. User clicks **Save & Enable**
3. **Pre-flight validation** runs synchronously (~5–10s): probes the model is available, every tool slug the agent will need is resolvable, every connection auth is live. **Blocks Save with a specific actionable error if any layer is broken** ("Composio Slack token expired — reconnect", "Model `claude-opus-4-7` not in your tier — try `agentic-v1`").
4. Workflow runs (manual or cron).
5. Run terminates.
6. The user sees a **per-run outcome card** in `/workflows/<id>/runs/<run_id>`:
   - ✅ "Sent email to alediez2408@gmail.com — Subject: *Morning brief 6/7*" — with **[Open in Gmail]** deep link
   - 📅 "Read 2 events from Google Calendar (range 6/7 08:00 → 6/7 23:59)"
   - 📨 "Read 7 unread Gmail messages from the last 24 hours"
7. If the run failed: outcome card shows a **failure-mode label** (`agent_narrated_without_acting` / `composio_upstream_rejected` / `model_unavailable` / `connection_expired`) with a one-liner explanation and, where possible, a fix-it button.

The user no longer needs to grep the SQLite DB, parse log files, or take a coding agent's word for it. The runtime *tells the truth* (F-16/F-21); the UI *shows the truth* (Phase 2.5).

---

## What Phase 2.5 is NOT

- **Not a model-routing fix.** `agentic-v1` works today; model selection is its own dimension and the per-workflow `model_tier` plumbing (executor.rs:3073-3079 deferred wiring) is a separate concern Phase 4 can address.
- **Not a Composio-API-shim layer.** Composio's upstream provider errors will still happen; Phase 2.5 surfaces them clearly but doesn't try to retry or paper over them.
- **Not Phase 4 Campaigns.** Trust UX is the substrate Phase 4 sits on — campaigns multiply per-workflow uncertainty by N, so Trust UX must precede it.
- **Not the deferred F-18 / F-19 frontend banners.** Those address the *connections* surface; Phase 2.5 is the *workflow run* surface. They share aesthetic principles but live in different routes.

---

## Tickets

| # | Title | Estimated effort | What it lands |
|---|---|---|---|
| **T-1** | Delivery-receipt sub-event | 2–3 days | When a Composio write tool (`*_SEND_*`, `*_CREATE_*`, `*_UPDATE_*`) returns success, emit + persist a structured `DeliveryReceipt { tool, side_effect_kind, recipient, message_id, link }` alongside the run. Backend foundation for T-2 and T-3. |
| **T-2** | Per-run outcome card UI | 3–4 days | Replace the raw agent-narrative-as-success-text with structured per-side-effect rendering in `/workflows/<id>/runs/<run_id>`. Render T-1 receipts as plain-English rows with deep links. |
| **T-3** | Pre-flight validation on Save & Enable | 3–4 days | Synchronous probe pipeline that validates model + tools + connections before persisting `enabled=true`. Replaces "Saved & Enabled then 400 on first run" with "blocks Save with specific actionable error". |
| **T-4** | Failure-mode catalog | 2–3 days | Stable `FailureReason` enum (`agent_narrated_without_acting` / `composio_upstream_rejected` / `model_unavailable` / `connection_expired` / `tool_slug_invalid` / `unknown`), persisted on every Failed run, rendered as one-liners + fix-it suggestions in the outcome card. |

Total: **10–14 working days** — roughly 2 calendar weeks for one focused implementer.

---

## Acceptance gate to start Phase 4 Campaigns

Trust UX is *blocking* for Phase 4. Specifically, Phase 4 cannot start until:

1. All 4 tickets shipped to `main` and verified end-to-end with the morning-email-brief workflow.
2. A 5-day cron streak runs without any silent failure (defined as: every Failed run carries a structured `FailureReason`; every Succeeded run carries at least one `DeliveryReceipt` if `allowed_connections` is non-empty).
3. The user has experienced the "click Run → see clean outcome card with [Open in Gmail] link → close laptop with confidence" loop without coming to a coding agent.

If 1–3 hold, Phase 4 starts on F4-1. If they don't, Trust UX is incomplete and Phase 4 waits.

---

## Files this phase will touch (estimate)

| File | Change |
|---|---|
| `src/openhuman/composio/tools.rs` | T-1: emit `DeliveryReceipt` on Composio success |
| `src/openhuman/workflows/types.rs` | T-1, T-4: add `DeliveryReceipt`, `FailureReason` to `RunStep` |
| `src/openhuman/workflows/executor.rs` | T-1: thread receipts through `NodeOutput`; T-4: classify terminal status into `FailureReason` |
| `src/openhuman/workflows/store.rs` + new migration | T-1, T-4: persist receipts (JSON column) + failure reason (TEXT column) |
| `src/openhuman/workflows/rpc.rs` | T-2: extend `workflows_get_run` response shape |
| `app/src/components/workflows/RunOutcomeCard.tsx` | T-2: new component |
| `app/src/pages/Workflows/RunDetail.tsx` | T-2: rewire run-detail to use outcome card |
| `src/openhuman/workflows/preflight.rs` (new) | T-3: probe pipeline |
| `src/openhuman/workflows/rpc.rs` | T-3: new `workflows_preflight` RPC |
| `app/src/components/workflows/preview/WorkflowProposalPreview.tsx` | T-3: call preflight before enabling |

---

## How to start

Tickets execute in this order (each depends on the previous):

1. **T-1** lays the data foundation — without `DeliveryReceipt` persisted, T-2 has nothing to render
2. **T-2** delivers the user-visible win first
3. **T-3** prevents future breakage at the gate
4. **T-4** rounds out failure-side trust

Each ticket has its own primer at `T-1.md` .. `T-4.md`. Read them in order before starting.
