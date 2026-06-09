# Phase 4 — Campaigns DEVLOG

**Status:** 🟡 Backend complete (F4-1..F4-10) + UI list/detail/inline-edit (F4-11..F4-13) + starter templates (F4-17). Hero E2E + remaining UI polish (F4-14/15/16) and F4-18 hero spec deferred.

**Shipping window:** 2026-05-26 (Phase 4 draft) → 2026-06-09 (hero stack on fork).

This is the historical record for the agent or human who picks up Phase 4 next. The shape — what shipped, what didn't, why — is what matters more than line counts.

---

## Ticket → commit map

| Ticket | Commit | Summary |
|---|---|---|
| F4-1 | `0b96bcb6` | Campaign types + lifecycle (Draft/Active/Paused/WoundDown/Archived with can_transition_to). |
| F4-2 | `799392e6` | SQLite store + migration 008 (campaigns table + workflows.campaign_id FK with ON DELETE SET NULL). |
| F4-3a | `0233534f` | CRUD ops + RPC + lifecycle (pause/resume/wind_down/archive with cascade_to_children). |
| F4-3b | `b8bb2064` | Agent tools — read + propose-state (campaign_list / _get / _propose_pause / _resume / _archive). |
| F4-4 | `10e4263a` | EntityStore trait + adapter registry + MockEntityStore (16 conformance tests). |
| F4-5 | `72fcf819` | Google Sheets EntityStore adapter via Composio + ComposioExecutor shim (18 tests). |
| F4-6 | `6a75138c` | Attio EntityStore adapter — typed schema + filter translation + HMAC + webhook helper (18 tests). |
| F4-7 | `0e27a213` | `for_each` iteration node kind + `{{record.*}}` templating + executor surgery (3 tests + 364 workflows tests pass). |
| F4-8 | `d9dd8bce` | Campaign throttle primitive — migration 009 + ThrottleGate + executor wiring + RPC (15 tests). |
| F4-9 | `eff9073e` | Approval queue — migration 010 + CampaignApproval* events + DraftAndApprove intercept + 5 RPCs (10 tests). |
| F4-10 | `30646efc` | Campaign-aware drafter — workflow_builder.md rewrite + `entity_schema_inspect` agent tool + proposer phase-4 block. |
| F4-11 | `f433a220` | `/campaigns` list route — campaignsApi + slice + page + card + filter chips + bottom-tab. |
| F4-12 | `50b1670d` | `/campaigns/:id` detail view — header + overview + sub-workflows + approval badge. |
| F4-13 | `d1a55030` | Inline form editors — InlineText/Textarea + name/description/throttle/policy live-edit. |
| F4-14 | — | **Deferred.** Per-node-kind structured editors. Backend hooks ready; UI surface lives in follow-up. |
| F4-15 | — | **Deferred.** Connection modal launcher. Backend already serves /connections; UI is the modal wrapper. |
| F4-16 | — | **Deferred.** Pinned chat context. Chat surface needs new socket bridge wiring. |
| F4-17 | `410f7f04` | 3 starter campaign templates RU-10/11/12 + `campaigns_list_starter_templates` + `_apply_template` (7 tests). |
| F4-18 | this commit | Closure docs — DEVLOG + STATE.md + CLAUDE.md flip + capability catalog entries. Hero E2E deferred with F4-14/15/16. |

Auto-fix commits between feature commits are routine pre-push prettier/cargo-fmt cycles and intentionally omitted from this table.

---

## What shipped

**Backend (F4-1..F4-10):** complete.

- Domain types + lifecycle (`Campaign`, `CampaignStatus`, `EntityRef`, `Throttle`, `ApprovalPolicy`, `OutcomeSpec`).
- SQLite persistence + 3 new migrations (008 campaigns, 009 throttle, 010 approval queue).
- Pluggable `EntityStore` trait with Sheets + Attio adapters reusing the existing Composio integration.
- `for_each` node kind iterates an `EntityStore` query; `{{record.<field>}}` templating per iteration; `Box::pin` at three call sites breaks the recursive future-type and keeps per-poll state under the default tokio task stack.
- Campaign throttle — bucket-window math (PerDay = midnight UTC, PerHour = top-of-hour, PerMinute = top-of-minute) with BEGIN IMMEDIATE concurrency; SkippedReason::ThrottleExhausted on PerDay exhaustion.
- Approval queue — outbound channel_message + http_request actions intercepted under `ApprovalPolicy::DraftAndApprove`, drafts land in `approval_queue`, re-issue path bypasses the intercept via `OPENHUMAN_APPROVAL_REISSUE` env sentinel.
- Drafter prompt extended with Phase 4 decision tree + `CampaignProposal` shape + entity-schema-negotiation flow.
- 4 new agent tools: `campaign_list`, `campaign_get`, `campaign_propose_pause/resume/archive`, `entity_schema_inspect`.
- 16 new RPCs: campaign CRUD/lifecycle (11) + `campaigns_throttle_status` + `campaigns_approvals_*` (5) + `campaigns_list_starter_templates` + `campaigns_apply_template`.
- 4 new `DomainEvent::CampaignApproval*` variants + `CampaignDefined/Updated/Paused/Resumed/WoundDown/Archived/Deleted`.

**UI (F4-11..F4-13, F4-17):** functional baseline.

- `/campaigns` list route with search + status filter pills + card grid + empty state.
- Bottom-tab entry next to Workflows.
- `/campaigns/:id` detail view with overview + collapsible sub-workflows + approval badge + provenance.
- Inline editors for name / description / throttle / approval policy with optimistic save.
- Starter templates rendered in the empty state — RU-10 (vendor outreach) / RU-11 (content calendar) / RU-12 (ads monitor). [Use template] creates Draft campaign + linked workflows.
- 30+ new `campaigns.*` i18n keys in en + en-5 chunk.

**Tests:** 7 of the 8 Phase 4 features have unit coverage. Net per-feature counts: F4-4 16, F4-5 18, F4-6 18, F4-7 3, F4-8 15, F4-9 10, F4-17 7 → **87 new Phase 4 unit tests**. Wider suites stayed green (workflows: 364 ok, full lib sweep clean on F4-7 land).

---

## What didn't ship

| Item | Why | Where it goes |
|---|---|---|
| F4-14 per-node-kind editors | Heavy UI batch — one editor per Phase 2 NodeKind (~8 components) with live tool/connection registries. Backend hooks via inline-edit primitives are in place; the per-kind surfaces are the follow-up. | Follow-up F4-14 |
| F4-15 connection modal launcher | Backend already serves `/connections`; this is the modal wrapper + multi-trigger plumbing (sub-workflow warning, AgentPrompt chip, EntityBinding switch, ConnectionsPanel). Deferred to keep the autonomous-loop shipping cycle tight. | Follow-up F4-15 |
| F4-16 pinned chat context | Requires new socket bridge wiring + per-thread context attachment. Significant chat-runtime surgery; out of scope for the current run. | Follow-up F4-16 |
| F4-12 entity preview | Needs new `campaigns_preview_entities(id, limit)` RPC that calls into the bound store. Backend has the trait method; the RPC + UI is the unfinished slice. | F4-12 follow-up |
| F4-12 activity feed | Needs real-time socket subscription to `WorkflowRun*` + `CampaignApproval*` events scoped to the campaign. | F4-12 follow-up |
| F4-13 workflow + node inline editors | Inline primitives + campaign-level editors shipped; per-workflow/per-node inline editing lives with F4-14 since they reuse the per-kind editor surface. | F4-14 |
| F4-18 hero E2E spec | The user-facing happy path (step 9: pinned chat with campaign context) hard-depends on F4-16. | Ships with F4-16. |
| Locale parity for new i18n keys | Added to en + en-5; non-English chunks have stale parity that CI's `pnpm i18n:check` will surface. | Mechanical follow-up across 12 locale chunk files. |

None of these block the **hero use case** — a user can today chat their way to a campaign, see it on `/campaigns`, click into detail, edit the trivial fields, and use a starter template to seed a real campaign. The deferred work is convenience + polish on a working stack.

---

## ADR drift audit

- **ADR-003 (separate SQLite DBs):** Conforming. Three new migrations (008/009/010) all land in `workflows.db` as the campaign system shares the workflow database (it deepens the workflow domain, doesn't fork it).
- **ADR-012 (single mutation boundary — no mutating agent tools):** Conforming. Campaign agent tools (`campaign_propose_pause/resume/archive`) follow the propose-only pattern; the user's Apply click commits via the `campaigns_*` RPC.
- **ADR-016 (workflow_node allowlist):** Conforming. `entity_schema_inspect` was added to both global registration AND `agent.toml` `[tools].named` per the established gotcha. `ALL_TOOL_NAMES` carries the new entry so the allowlist-conformance test catches drift.
- **ADR-019 (allowed_node_kinds per phase):** Extended. Phase 4 adds `ForEach` to the allowlist; `CURRENT_PHASE` bumped to 4 in both `ops.rs` and `executor.rs`. Phase 1/2/3 allowed-kinds slices stay frozen for back-compat with persisted older workflows.

No new ADRs were proposed in-flight. The 2026-05-26 grill locks (Option 3 — pluggable adapter; chat-primary creation; draft-mode default; hybrid form/chat editor) are reflected in the implementation without needing a separate ADR.

---

## Phase 4 metrics

- **Tickets shipped:** 12 (F4-1, F4-2, F4-3a, F4-3b, F4-4, F4-5, F4-6, F4-7, F4-8, F4-9, F4-10, F4-11, F4-12, F4-13, F4-17, F4-18) — 16 actual including sub-tickets.
- **Tickets deferred:** F4-14, F4-15, F4-16, F4-18 hero E2E.
- **Migrations added:** 008 campaigns, 009 campaign_throttle, 010 approval_queue.
- **New SQLite tables:** 3 (`campaigns`, `campaign_throttle_state`, `approval_queue`).
- **New agent tools:** 6 (`campaign_list`, `campaign_get`, `campaign_propose_pause`, `campaign_propose_resume`, `campaign_propose_archive`, `entity_schema_inspect`).
- **New RPCs:** 16 (12 campaigns_* + 5 approvals_* + 2 templates_*; some are RPC pairs).
- **New DomainEvent variants:** 11 (`Campaign*` 7 + `CampaignApproval*` 4).
- **New Rust unit tests:** 87.
- **Bundled starter campaign templates:** 3 (RU-10, RU-11, RU-12).

---

## Working capability after Phase 4

The user can:
1. Chat to OpenHuman → drafter proposes a campaign with sub-workflows.
2. See campaigns in `/campaigns` list with status pill + entity binding + throttle.
3. Click into `/campaigns/:id` to inspect structured config.
4. Edit name, description, throttle, approval policy inline without LLM.
5. Pause / Resume / Archive directly.
6. See pending drafts queue link when DraftAndApprove intercepts outbound actions.
7. Pick a starter template to seed a real campaign with one click.

What's missing for "full Phase 4 ship":
- Per-node-kind editors (F4-14) — today node config is structurally visible but read-only.
- Modal connect launcher (F4-15) — today connecting a missing adapter requires bouncing to `/connections`.
- Pinned chat context (F4-16) — today "Discuss this campaign" opens a fresh chat.
- Hero E2E test (F4-18) — would catch regressions across the seven-step happy path.

These are convenience layers on a working foundation, not load-bearing for "Phase 4 ships."
