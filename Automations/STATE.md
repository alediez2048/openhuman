# Workflows & Automations — Current State

**Last updated:** 2026-06-09 (Phase 4 backend + UI baseline shipped to fork)
**Branch:** `main` on `alediez2048/openhuman` (the user's fork). Upstream `tinyhumansai/openhuman` not pushed to yet — this is private dev so far. Phase 1 + 2 rollup PR is the next upstream push.

A fresh session should read this file first to know where the initiative stands.

---

## TL;DR

**Phase 0 (Connections Hub) is SHIPPED** to `alediez2048/main`. Unified `/connections` with all 6 mechanisms + honest verification.

**Phase 1 (Workflows Foundation) is SHIPPED** to `alediez2048/main`, including the Phase 1.5 polish that locked the chat-driven create flow end-to-end.

**Phase 2 (Execution Depth) is SHIPPED** to `alediez2048/main` on 2026-05-26 (F2-1 → F2-17 + F2-7b + F2-9b + F2-16b). Multi-node chains, webhook/composio/channel triggers, retry policy, active_hours gate, 30-day soft-delete + retention sweep, 5 starter templates RU-5..RU-9, Appium live-test spec. See `Automations/Tickets/phase-2-execution/DEVLOG.md` for the full closure log.

**Implementation order (revised 2026-05-26):** Phase 2 ✅ → **Phase 4 (Campaigns)** → Phase 3 (Browser Agent) → Phase 5 (Business Entities) → Phase 6 (Proactive Agent). Phase 4's slot was previously held by Canvas but Canvas was demand-gated and the demand never materialised; the 2026-05-26 grill replaced it with **Campaigns + Workflow UX** which is the user's actual ask. Canvas drafts preserved at `phase-4-canvas/` but marked superseded.

**🟡 Phase 4 (Campaigns) backend + UI baseline SHIPPED to fork on 2026-06-09.** Hero stack landed in 12 ship cycles: F4-1 (types) → F4-2 (store) → F4-3a/b (ops + RPC + agent tools) → F4-4 (EntityStore trait) → F4-5 (Sheets) → F4-6 (Attio) → F4-7 (`for_each` executor) → F4-8 (throttle) → F4-9 (approval queue) → F4-10 (drafter prompt + `entity_schema_inspect`) → F4-11 (`/campaigns` list) → F4-12 (`/campaigns/:id` detail) → F4-13 (inline editors) → F4-17 (3 starter templates RU-10/11/12) → F4-18 (closure docs).

User can today: chat → drafter proposes a campaign → see it on `/campaigns` → click into detail → edit trivial fields inline → pause/resume/archive → use a starter template for one-click campaign creation → drafts land in `/approvals` under the DraftAndApprove policy.

**Deferred follow-ups (not load-bearing for the hero use case):** F4-14 per-node-kind editors, F4-15 connection modal launcher, F4-16 pinned chat context, F4-18 hero E2E (depends on F4-16), entity preview RPC + activity feed on the detail view, non-English locale parity for the new i18n keys. See `phase-4-campaigns/DEVLOG.md` for the full ticket-by-ticket map + ADR drift audit + metrics.

Locked architecture decisions from the 2026-05-26 grill that shipped:
1. Conversation handling = (B) Draft mode first. ✅
2. Architecture shape = β — Campaigns as first-class entity owning workflows. ✅
3. Entity store = Option 3 — pluggable `EntityStore` trait, Google Sheets + Attio adapters at MVP. ✅
4. Creation surface = Option Y — chat-primary, Canvas explicitly deferred. ✅
5. Workflow detail editor = (D) Hybrid — form for trivial fields. ✅ Chat-for-non-trivial deferred to F4-16.
6. Connection-add UX = (i) Modal launch. ⏳ Deferred to F4-15.
7. Chat-panel for updates = (y) Pinned workflow context in global chat. ⏳ Deferred to F4-16.

**🟡 NEXT WORK:** either close the remaining Phase 4 UI polish (F4-14/15/16/18 hero spec) OR move to Phase 3 (Browser Agent) per the original ordering.

**Phase 3.1 (Browser Agent) — backend shipped to fork on 2026-06-12.** Drafted tickets `F3-1` through `F3-7` + `F3-4.5` under `Automations/Tickets/phase-3-browser-agent/`. The thesis is a CEF-native CDP-driven browser agent (Stagehand-style `act`/`extract`/`observe`) that drives the user's already-authenticated webview sessions. Additive to Composio, not a replacement.

Shipped to fork:
- **F3-1 ✅** CDP automation primitives (Rust).
- **F3-2 ✅** Page perception — DOM extractor with `[contenteditable]` selector + `aria-placeholder` accessibleName fallback (live-tested against LinkedIn's post composer).
- **F3-3 ✅** `browser_observe` / `browser_act` / `browser_extract` LLM tools.
- **F3-4 ✅** `NodeKind::BrowserAction` workflow node + validator + executor dispatch.
- **F3-4.5 ✅** Live `WsTransport` against CEF debug port + profile-aware session opener (creates a tab from stored cookies when no live tab matches the provider).
- **F3-5 chunks 1 + 2a ✅** Rust-side preview broadcaster + `DomainEvent::BrowserPreviewFrame` + socket bridge that publishes `browser_preview_frame` events on the web channel for the frontend to consume.
- **F3-6 chunks 1 / 2 / 3 / 4a ✅** Safety preamble + dry-run mode + per-tool-call audit log (with retention sweep) + wall-clock cost cap (validator-clamped [30, 3600], default 600s) + text/URL redaction policy applied to audit-log args.

Hero use case ("post 'Good Morning Everyone' to LinkedIn via browser automation") **is technically operable end-to-end on the fork** as of 2026-06-12 — drafter emits browser_action, validator accepts, opener attaches (auto-creates tab from cookies if needed), agent loop drives the page via observe/act/extract, audit log captures every action with redaction, wall-clock cap prevents runaway spend, dry-run mode short-circuits writes. Smoke-tested live against LinkedIn.

Deferred to Phase 3.2 (`Automations/Tickets/phase-3-browser-agent/PHASE-3-2-DEFERRED.md` placeholder):
- **F3-5 chunk 2b** — React `BrowserPreviewPanel` component + Tauri IPC subscription (Rust-side bridge in place; only the frontend consumer is missing).
- **F3-6 chunk 4b** — screenshot pixel-level redaction (black-bar overlay on password input bounds; needs the `image` crate dep).
- **F3-6 chunk 4c** — per-action confirmation gate (depends on F3-5 chunk 2b for the Confirm/Reject UI surface).
- **F3-7** — vision-grounded fallback (Anthropic computer-use style, opt-in).
- **Webview warmth** — auto-open the provider's webview on cron-triggered runs without active user presence. Currently the opener creates a fresh tab from cookies when none exists, which covers manual-run cases.
- **Drafter dry-run-by-default** for newly-created browser_action workflows (training-wheels pattern; F3-6 chunk 2 follow-up).

Notable runtime bug fixes shipped alongside Phase 3.1 (all to fork): `workflows::ops::CURRENT_PHASE` unified across executor + propose tools (was rejecting `browser_action` as `unsupported_node_kind`), orchestrator prompt updated to list every current `NodeKind` (was pre-emptively refusing browser-agent requests), `should_honour_model_pin` predicate (was sending OpenHuman tier names like `agentic-v1` to direct Anthropic providers → 404).

**Canvas (originally Phase 4) SUPERSEDED** by Phase 4 Campaigns. Drafts preserved at `Automations/Tickets/phase-4-canvas/` for reference but the Phase 4 slot now belongs to Campaigns. Canvas remains demand-gated per `prd.md §5.3`; Phase 4's hybrid detail editor (F4-12..F4-16) may make Canvas permanently unnecessary.

**Phase 5 (Structured Business Entities + Outcome Observability) PLACEHOLDER** at `Automations/Tickets/phase-5-business-entities/PLACEHOLDER.md` — not drafted. Forward-compat hook landing in F-17 via `entity_tags` so the structured layer can be built later from real data. Earliest start: after Phase 3 ships AND 3–6 months of `entity_tags` data has accumulated in the Memory Tree. Schemas emerge from observation, not theory. Surfaced by the 2026-05-22 grill when the user's vision pivoted from personal-productivity to "autonomous business-growth agent" — leads, deals, proposals, payments need structured queryability that Memory Tree's chunk model doesn't natively support.

**Phase 6 (Proactive Agent — agent-initiated workflows) PLACEHOLDER** at `Automations/Tickets/phase-6-proactive-agent/PLACEHOLDER.md` — not drafted. This is the long-term North Star confirmed in the 2026-05-22 grill: every prior phase is reactive (triggers fire → workflow runs); Phase 6 is the proactive layer where the agent NOTICES patterns in the user's Memory Tree + Phase-5 entities and PROPOSES workflow runs of its own initiative. Per-pattern trust gradient (L0 supervised default → L1 auto-fire-with-notification after N consecutive approvals → L2 silent — all user-controlled, per-pattern, revocable). Earliest start: after Phase 5 has been live for 2–4 months. Captured now so the architectural shape of Phases 2-5 stays compatible with the proactive endpoint.

---

## What's live on `main` today

### Phase 1 deliverables (F-1 through F-21 + Phase 1.5 polish; all landed)

| Surface | Status | Where |
|---|---|---|
| `/workflows` route + bottom-tab | Shipped | `app/src/pages/Workflows/WorkflowsList.tsx`, `BottomTabBar.tsx` |
| Starter catalog (RU-1..RU-4) | Shipped | `src/openhuman/workflows/templates/*.json` |
| All 12 mutating + read RPCs | Shipped | `src/openhuman/workflows/{rpc,schemas,ops}.rs` |
| Health recompute on connection events | Shipped | `workflows/bus.rs` |
| Cron + manual scheduler | Shipped | `workflows/scheduler.rs` |
| Executor + run history | Shipped | `workflows/executor.rs` |
| Single-flight + soft-cancel + orphan-recovery | Shipped | `workflows/executor.rs` (F-9) |
| 4 read-only + 6 propose-only agent tools | Shipped | `tools/impl/workflows/*` |
| Drafting sub-agent + validator + retry | Shipped | `workflows/{proposer,validator}.rs` |
| `workflow_builder.md` bundled | Shipped | `agent/prompts/workflow_builder.md` + Tauri resources |
| Preview components (Proposal/Edit/Delete/State) | Shipped | `app/src/components/workflows/preview/*` |
| Hero-flow chat-runtime extension (Phase 1.5) | Shipped | `<workflow-preview>` tag parsed in `AgentMessageBubble` |
| Real `Agent::from_config().run_single()` in drafters (Phase 1.5) | Shipped | `workflows/proposer.rs` |
| Orchestrator allowlist exposes workflow tools (Phase 1.5) | Shipped | `agent/agents/orchestrator/agent.toml` |
| Wildcard-aware connection matching (Phase 1.5) | Shipped | `workflows/health.rs::matches_ref` |
| Channel/Webview send stubs (Phase 1.5 deferral) | Stub | `tools/impl/workflows/{channel_send_stub,webview_account_send_stub}.rs` |
| Catalog flow E2E spec (NFR-2.6.4) | Shipped | `app/test/e2e/specs/workflows-seeded.spec.ts` |
| Hero flow E2E spec (NFR-2.6.3) | Deferred | Documented in Phase 1 README as the next E2E ticket |
| Phase 1 capability entries | Shipped | `about_app/catalog.rs` |
| Phase 1 README + DEVLOG closure + ADR drift audit | Shipped | `Automations/Tickets/phase-1-foundation/{README,DEVLOG}.md` |

### What's testable end-to-end TODAY

1. **Catalog flow** — open `/workflows` → 4 starter cards → click [Add & Enable] on RU-1 → workflow row appears → catalog dedupes the template → delete → catalog re-shows the template. Fully wired.
2. **Workflow card overflow actions** — Run now (with health gating), Edit (inline message pointing at chat), Delete (with "Move to starter workflows" labeling for Seed-origin rows).
3. **Chat-driven creation for Composio-routed workflows** — type "build me a workflow that..." in `/chat` → orchestrator calls `workflow_propose_create` → drafting sub-agent invokes the real LLM → fenced ```json``` parsed into a `WorkflowProposal` → tool returns `<workflow-preview>` tag → `AgentMessageBubble` parses + dispatches → `WorkflowProposalPreview` renders → click [Save & Enable] → workflow lives.
4. **Manual run** of any Phase-1-shape workflow (single `agent_prompt` node, Composio-routed connections) → `Agent::from_config().run_single()` produces real output → run row + step row persisted.
5. **Run history** — `workflows_list_runs` / `workflows_get_run` RPCs + agent tools.
6. **Boot-time health recompute** — workflows whose health was computed under old matching rules get refreshed against the live snapshot on next boot (`recompute_all_workflows`).

### What's a known Phase 1.5 / Phase 2 deferral

- **✅ F-16 (LANDED `3b572f71`, 2026-05-22) — workflow executor enforces ADR-016 allowlist + honest step status.** Closed the executor-side placeholder F-15 left behind: workflow runs now spawn a constrained `workflow_node` sub-agent (no orchestrator persona, no `delegate_*`) with the per-run `def.allowed_tools` allowlist as the override. `composio_execute` is now the obvious LLM choice for the Composio-routed Gmail / Slack / etc. surface — the orchestrator-identity leak that caused 2026-05-21 22:13's silent Slack failure is closed. Step status is honest: a `ToolExecutionCompleted{success:false}` observed during the run forces `RunStatus::Failed` with a clear summary, even when the agent itself returned text. Composio-routed workflows now actually run end-to-end.
- **✅ F-17 (LANDED locally 2026-05-22) — workflows wired into the Memory Tree.** Closes the memory↔doer loop gap. Three executor hooks: (1) pre-run recall fetches up to 3 prior-run summaries from `workflow/{workflow_id}` and prepends them to the user-message prompt as a `## Prior runs of this workflow` Markdown block (or `## No prior runs — this is the first execution.` on first run); (2) F-16's `ToolExecutionCompleted` subscriber upgraded from counter-only to per-call `Vec<ToolCallObservation>` so the post-run builder carries detail, not just a failure count; (3) `persist_run_memory(...)` writes a structured `WorkflowRunMemory` chunk after every run with `actual` (from the F-16 trace), `narrative` (agent text), F-16-honest `status`, `narrative_drift` (regex/substring heuristic that catches "agent claims sent but all tool calls failed"), and `entity_tags` (auto from `allowed_connections` + the agent's optional `## Entities touched` section). Memory write is best-effort; failure does NOT roll back the terminal status. Namespace is `workflow/{id}` (slash not colon — `UnifiedMemory::sanitize_namespace` strips `:`). 19 new unit tests in `memory.rs` + 3 integration tests in `executor_tests`. All 202 workflow tests green.
- **✅ F-18 (LANDED 2026-05-23, `f7539a2e`) — MCP server registration is user-isolation safe.** Stale-handle guard in `connections::ops` prevents the split-brain where a registration lands in the wrong `users/{id}/config.toml` after an active-user swap. Orphan scanner (`connections_mcp_orphans_list` RPC) walks `users/*/config.toml` to surface previous-session MCP servers with secrets redacted; `connections_mcp_orphans_migrate` accepts an orphan into the active user's config. Frontend banner on `/connections` is deferred — backend RPCs are ready.
- **✅ F-19 (LANDED 2026-05-23, `f7539a2e` + `dbb96707`) — MCP UX hardening.** Two parts shipped: (a) structured tool errors — `McpToolErrorKind` enum + `classify_mcp_error` + `render_mcp_tool_error` produce the verbatim-render `⚠ MCP tool error\nkind: ...\ndetail: ...\nsuggestion: ...` block; orchestrator prompt teaches the LLM to surface this verbatim instead of confabulating HTTP codes / OAuth scope names. (b) endpoint auto-probe on add — `probe_mcp_endpoint` tries `/`, `/mcp`, `/sse`, `/messages` and corrects the saved endpoint when only one path responds to `initialize`. Part 3 (curated MCP catalog) deferred. Also bundled: UI rename "HTTP" → "API / HTTP" tab + show/hide toggle on bearer/credential inputs.
- **✅ Composio event bridge fix (`fac57af4`, 2026-05-23) — workflow health recompute now fires on Composio connect/disconnect.** Pre-fix the `ComposioConnection{Created,Deleted}` events weren't bridging to the unified `DomainEvent::Connection{Added,Removed}` family, so workflow health stayed stale after a Gmail/Slack/etc. reconnect.
- **✅ SQLite contention fix (`7e3bca21`, 2026-05-23) — memory tree ingest no longer livelocks on busy reads.** `PRAGMA synchronous=NORMAL` + `BEGIN IMMEDIATE` for `persist()` transactions (replacing `unchecked_transaction`). Closes the gmail-ingest "database is locked" errors observed under concurrent reads.
- **✅ F-20 (LANDED 2026-05-24, `9f65be25`) — integrations_agent: slug-shape validation + structured Composio tool errors.** Same disease F-19 fixed for MCP, now on the chat orchestrator's `integrations_agent → composio_execute` path. Five parts: (1) pre-dispatch regex `^[A-Z][A-Z0-9]*(_[A-Z0-9]+)+$` in `ComposioExecuteTool::execute` rejects toolkit names (`linkedin`, `composio`, `gmail`) before they hit the backend; (2) `composio/tool_errors.rs` with `ComposioToolErrorKind` (10 variants) + `classify_composio_error` + `render_composio_tool_error` mirroring F-19's contract; (3) orchestrator prompt renamed to "MCP **and Composio** tool failures" with both shapes shown; (4) integrations_agent prompt gains wrong-vs-right slug example + "if asked to act, act" rule + verbatim-pass-through rule; (5) `composio_list_toolkits` added to integrations_agent allowlist. 15 new tests; 522 composio tests + 33 agent loader green.
- **✅ F-21 (LANDED 2026-05-24, `29e662f5`) — F-19/F-20 hardening trio.** Three fixes from the post-F-20 review: (1) `probe_mcp_endpoint` now does no-auth probe first; the bearer only attaches when the response shape proves MCP (JSON-RPC body, Bearer challenge with JSON body, etc.) — prevents token leak to typo'd hosts or admin panels; (2) shared verbatim-render fragment at `agent/prompts/structured_tool_errors.md` with loader-test enforcement that every agent whose allowlist contains `mcp_*` / `composio_execute` includes it (caught planner as an offender on first run); (3) drift telemetry — `classify_mcp_error` / `classify_composio_error` both call `observability::record_classifier_drift(source, detail)` on `Unknown` returns, bumping an atomic counter AND emitting a sentry-bound `tracing::warn!(target: "tool_error_drift", …)`. 17 new tests; 524 composio + 61 connections + 68 agent + 12 mcp tests green. **Phase 2 starts next.**
- **Channel send + Webview send** — `channel_send` and `webview_account_send` tools are stubs returning "Phase 2 (F2-5) deferral" errors. Workflows touching Channel or Webview connections in their `allowed_connections` won't actually send messages; they'll fail loud with a clear reason (and now also flip the run to `Failed` honestly per F-16 D, instead of lying as `Succeeded` like before). **Composio-routed channels work** (Slack, Discord, Telegram, etc. via Composio's `composio_execute`). Land F2-5 to unify Channel/Webview send.
- **Multi-node chains** — executor rejects `nodes.len() != 1` for Phase 1. F2-2 lands it.
- **Phase 2 trigger types** — webhook, composio_event, channel_message. F2-9/F2-10/F2-11.
- **Phase 2 node kinds** — tool_call, http_request, channel_message, condition, delay. F2-3..F2-7.
- **Hero E2E spec** — `workflows-agent-creation.spec.ts` per F-15's original deliverable. The components + agent invocation are all wired today; the E2E spec authoring is the missing piece.
- **30-day soft-delete + retention sweep** — F-2 hard-deletes today; FR-1.3.4 retention sweep deferred to F2-14.
- **`active_hours` enforcement on cron** — F-7 ignored the field; F2-15.
- **Visual canvas + transform/await/fan_out** — Phase 4.

---

## Phase 1.5 polish — landed this session (2026-05-21)

Commits that closed the original F-15 deliverables I'd previously marked deferred:

| Commit | Subject |
|---|---|
| `eea486f5` | Real agent invocation in drafters + chat-runtime preview rendering |
| `ca7accba` | Wire WorkflowCard overflow Run / Edit / Delete actions |
| `e6ae9ecc` | "Move to starter workflows" labeling for Seed-origin delete |
| `f0a2288c` | Wildcard match for empty `account_id` / `channel_id` in `is_connected` |
| `7a10562c` | Persistent "Build a workflow" CTA + Show starter toggle |
| `23645a25` | Orchestrator prompt teaches the chat agent about the Workflows feature |
| `4c54e649` | Expose workflow tools in the orchestrator's `named` allowlist (root cause for "agent doesn't see my tools") |
| `b0e3b73c` | Register `channel_send` + `webview_account_send` stub tools (F-8 named them; never registered) |
| `1445afb5` | Refresh proposer module doc — placeholder body is gone |

These were surfaced by a debugging session the user kicked off after testing revealed the agent couldn't find the workflow feature. Two material gaps were found by parallel investigation agents:

1. **Orchestrator's `named` allowlist filtered out the workflow tools** even though F-10/F-12 registered them globally via `tools::ops::all_tools_with_runtime`. The `[tools].named = [...]` array in `agent.toml` is an explicit whitelist, not a fallback. Fix: add the 10 names + an inline ADR-012 reminder.
2. **`channel_send` / `webview_account_send`** were named by F-8's `build_node_agent_definition` but never had `Tool` impls. Workflows touching Channel/Webview connections would have failed with "tool not registered" at run time. Fix: register Phase-2-deferral stubs that return a clear error rather than crashing the agent.

---

## Drafted phase ticket sets (Phase 2 / 3 / 4 / 5 / 6)

Drafted in commit `90e4b7d6`.

**Phase 2 — `Automations/Tickets/phase-2-execution/`** — 16 tickets, ~75h:
- F2-1..F2-2: Scaffold + multi-node execution
- F2-3..F2-7: Per-node-kind impls (tool_call/http_request/channel_message/condition/delay)
- F2-8: on_error + retry
- F2-9..F2-11: webhook/composio_event/channel_message triggers
- F2-12: RU-5..RU-9 templates
- F2-13: Prompt update
- F2-14: 30-day soft-delete sweep
- F2-15: active_hours enforcement
- F2-16: Hero + catalog E2E + closure

**Phase 3 — `Automations/Tickets/phase-3-browser-agent/`** — overview + 7 sub-tickets, ~23–33 working days for Phase 3.1. The next priority after Phase 2:
- F3-overview: thesis + 5 architectural forks + capability gap analysis + reference-repo notes
- F3-1: CDP automation primitives (Rust) — 3–5 days
- F3-2: Page perception (DOM + a11y tree grounding) — 3–4 days
- F3-3: LLM-facing tools (browser_observe / browser_act / browser_extract) — 4–6 days
- F3-4: Workflow node integration (`NodeKind::BrowserAction`) — 2–3 days
- F3-5: Live-preview UI surface — 4–5 days
- F3-6: Safety preamble + dry-run + cost caps + audit log — 3–4 days
- F3-7: Vision-grounded fallback (Anthropic computer-use style, opt-in) — 4–6 days

Phase 3.2 (cloud Chromium / Playwright sidecar) is captured in F3-overview's "explicitly deferred" section. Do not start Phase 3.2 until Phase 3.1 is on `main`.

**Phase 4 — `Automations/Tickets/phase-4-canvas/`** — 10 tickets, ~60h. **Demand-gated** per `prd.md §5.3`; the current chat-driven creation + expanded list-row workflow card may already cover the need:
- F4-1: @xyflow/react integration + read-only render
- F4-2..F4-3: Palette + per-node config drawer
- F4-4..F4-5: Edge wiring/DAG + live run highlighting
- F4-6..F4-8: transform/await_human_approval/fan_out node kinds
- F4-9: Canvas-driven create flow
- F4-10: Hero E2E + closure

Each phase ships a README index listing open OQs to resolve in the pre-phase brainstorm before starting ticket #1.

---

## Pre-existing test failures (NOT ours; verified against clean `main`)

These predate the F-17..F-21 work and are confirmed reproducible on `29e662f5` with all uncommitted changes stashed:

1. **`agent::harness::session::turn::turn_uses_cached_transcript_prefix_on_first_iteration`** — test-isolation bug: reads the developer's real `~/.openhuman/users/.../memory.db` instead of an isolated tempdir, so the assertion's expected `"fresh"` body gets the user's actual memory dump prepended. Passes on a clean workspace / CI.
2. **`agent::harness::subagent_runner::ops::tests::typed_mode_blocks_unallowed_tool_calls`** — stack-overflow panic during the test. Reproducible deterministically on this machine; likely the same dev-env-sensitive class as #1.
3. **`tools::network::polymarket::place_order_happy_path`** — mock-server contract drift. Passes under plain `cargo test`, fails under the `test:rust` mock wrapper.

**Domain-scoped sweeps are clean** — `cargo test --lib workflows::` / `connections::` / `composio::` / `memory::tree` / `agent::agents::loader` all pass green. The two failures above only surface in `cargo test --lib` (full sweep) on this developer machine. Phase 2 work should use the per-domain commands to verify changes; address the test-isolation bugs as a follow-up cleanup ticket if they start blocking CI.

---

## Gotchas learned across Phase 0 + Phase 1

### Phase 0 (pre-existing)

- **Aggregator collectors must rebuild source registries per call** — never hold `Arc<Registry>` snapshots in tools or services.
- **MCP HTTP clients MUST send `Accept: application/json, text/event-stream`** — spec-strict servers return 406 without it.
- **Telegram Web stores auth in IndexedDB**, not cookies.
- **CEF cookies don't flush synchronously.** Modals poll while open + after close.
- **`RpcOutcome::single_log` wraps responses in `{ result, logs }`** — frontend API clients must unwrap. `connectionsApi.ts` + `workflowsApi.ts` both have helpers.

### Phase 1 + Phase 1.5

- **The orchestrator's `agent.toml` uses an EXPLICIT `named` allowlist for tools.** Registering a tool globally in `tools::ops::all_tools_with_runtime` does NOT expose it to the chat agent. Every new agent-callable tool must also land in the orchestrator's whitelist. Same for planner / integrations_agent / etc. — each agent has its own toolscope.
- **Agent prompts are captured at thread/session start.** After changing `orchestrator/prompt.md` or `agent.toml`, start a new chat thread (not just reload the page).
- **F-8's `build_node_agent_definition` names tools by string** (`composio_execute`, `channel_send`, `webview_account_send`, `mcp_call_tool`, `http_request`, `builtin_<integration>`). If a name doesn't have a registered `Tool` impl, the agent_prompt sub-agent will fail with "tool not registered" at run time, not at validation time. Audit the allowlist whenever F-8's `connection_tool_name` function changes.
- **`ConnectionsSnapshot::is_connected` does wildcard matching** for empty `account_id` / `channel_id` / `tool_name` — starter templates use this convention because they don't know the user's specific id at bundle time. Cross-mechanism mismatches (Channel vs Webview) are NOT wildcarded — they're different integrations.
- **Agent invocation from non-Turn contexts** = `Agent::from_config(config).run_single(composed_prompt)` (the cron-domain pattern). `subagent_runner::run_subagent` errors with `NoParentContext` outside a harness turn. Compose the system prompt + user message into one string; the agent treats it as user input.
- **Chat-runtime parses `<workflow-preview kind="..." data='{json}'></workflow-preview>` tags** in `AgentMessageBubble` via `parseBubbleSegments`. Propose tools emit the tag in their `success_with_markdown` body + advertise `supports_markdown=true` so the harness picks it up + the orchestrator echoes the tag verbatim.
- **Phase 1 starter templates assume Channel-mechanism Telegram (bot API).** Users with Webview Telegram (browser session) won't satisfy the requirement even with wildcard matching, because variant must match.
- **Don't unilaterally scope-cut tickets and label deferrals as "Phase X.5" without permission.** F-15's hero E2E was a hard deliverable, not a "Phase 1.5". When the budget genuinely doesn't fit, ask before deferring.
- **The `vendored cargo-tauri` install path** (`.cache/cargo-install/bin/cargo-tauri`) isn't on the default `PATH`. Symlink it into `~/.cargo/bin/cargo-tauri` so `cargo tauri dev` resolves.

---

## What a fresh session should do first

**Default next action: start Phase 2.** F-17, F-18, F-19, F-20, F-21 all landed locally; Phase 1.5 is complete. Domain-scoped test sweeps all green: 524 composio + 202 workflows + 61 connections + 602 memory_tree + 68 agent + 12 mcp.

1. Read this file (`Automations/STATE.md`) to know where the initiative stands.
2. Read `CLAUDE.md` for the repo-level commands + conventions.
3. Verify the regression baseline still holds with per-domain sweeps (full `cargo test --lib` has two pre-existing dev-env-sensitive failures — see the section above): `cargo test --lib workflows::` (202), `cargo test --lib connections::` (61), `cargo test --lib composio::` (524), `cargo test --lib memory::tree` (602), `cargo test --lib openhuman::agent::agents::loader` (68). Plus `cargo test --lib network::mcp` (12).
4. Read `Automations/Tickets/phase-2-execution/README.md` end-to-end. Resolve the 5 brainstorm OQs (OQ-4 triggers / OQ-5 retention / OQ-7 inter-node data / OQ-13 retry shape / OQ-14 webhook payload) — leans are documented in the README; lock each into `Automations/requirements.md §8` before committing F2-1 code.
5. Read `Automations/Tickets/phase-2-execution/F2-1.md` end-to-end. F2-1 is "make Phase 2 NodeKind + Trigger variants reachable" — declarative scaffold; F2-3..F2-7 fill the per-kind execution bodies.
6. Optional context: skim F-17 closure in DEVLOG.md (memory loop you're building on top of) + F-18 / F-19 / F-20 / F-21 (MCP + Composio hardening — backend-ready with frontend banner/notice deferred, not blockers for Phase 2).
7. Start F2-1. The brainstorm OQs gate the architecture; once locked, F2-1 commit-then-test cadence picks up.

**Deferred follow-ups that are NOT Phase 2 prereqs but worth knowing exist:**
- F-18 frontend orphan banner on `/connections` (backend RPCs ready: `connections_mcp_orphans_list` + `connections_mcp_orphans_migrate`).
- F-19 frontend probe-result notice in MCP Add modal (backend returns auto-correction in `RpcOutcome.logs`).
- F-19 Part 3 curated MCP catalog (~10 popular MCP servers with defaults baked in).
- F-20 follow-up: structural "no-tool-call iteration counter" to catch the "agent returns text instead of acting" pattern (today prompt-level discipline only).
- F-21 deferred: generic `OrphanScanner<T>` / `ConnectionProbe<T>` extraction (worthwhile when the next connection kind grows user-scoped configs).
- Pre-existing test-isolation cleanup: the two `agent::harness::session::turn` / `subagent_runner::ops::typed_mode_blocks_unallowed_tool_calls` failures should land their own one-off ticket so `cargo test --lib` is clean.

If a USER tasks you with a Phase 1 bug instead of Phase 2: re-check the "What's a known deferral" section in this file before assuming regression — many "missing" features are documented deferrals.

**Do NOT start Phase 3 (browser agent), Phase 4 (canvas — demand-gated), Phase 5 (entity tables — placeholder), or Phase 6 (proactive agent — placeholder) without explicit user direction.** Their tickets exist as planning artifacts, not implementation targets.

---

## Critical files to know

| File | Why |
|---|---|
| `src/openhuman/workflows/` | Full Phase 1 backend |
| `src/openhuman/tools/impl/workflows/` | 10 read + propose tools + 2 send stubs |
| `src/openhuman/agent/agents/orchestrator/agent.toml` | Orchestrator's tool allowlist (must include workflow tools) |
| `src/openhuman/agent/agents/orchestrator/prompt.md` | Orchestrator's system prompt (must teach the agent about workflows) |
| `src/openhuman/agent/prompts/workflow_builder.md` | Drafting sub-agent's system prompt |
| `app/src/components/workflows/` | UI components |
| `app/src/pages/Workflows/WorkflowsList.tsx` | `/workflows` route |
| `app/src/pages/conversations/components/AgentMessageBubble.tsx` | Parses `<workflow-preview>` tags |
| `app/src/pages/conversations/utils/format.ts` | `parseBubbleSegments` includes the tag matcher |
| `Automations/Tickets/phase-2-execution/F2-1.md` | **The next implementation primer — read first when starting work** |
| `Automations/Tickets/phase-2-execution/README.md` | Phase 2 ticket index + 5 brainstorm OQs to resolve before F2-1 |
| `Automations/Tickets/phase-1-foundation/F-17.md` | F-17 memory loop primer (closure context Phase 2 builds on) |
| `Automations/Tickets/phase-1-foundation/F-18.md` | F-18 MCP user-isolation safety (orphan migration RPCs ready; UI banner deferred) |
| `Automations/Tickets/phase-1-foundation/F-19.md` | F-19 MCP UX hardening (structured tool errors + endpoint auto-probe) |
| `Automations/Tickets/phase-{1-foundation,2-execution,3-browser-agent,4-canvas,5-business-entities,6-proactive-agent}/` | Per-phase ticket sets (Phase 5/6 are placeholders only) |
| `Automations/ADRs/` | 20 ADRs locked across the initiative |
