# Workflows & Automations — Current State

**Last updated:** 2026-05-21
**Branch:** `main` on `alediez2048/openhuman` (the user's fork). Upstream `tinyhumansai/openhuman` not pushed to yet — this is private dev so far. Phase 1 rollup PR is the next upstream push.

A fresh session should read this file first to know where the initiative stands.

---

## TL;DR

**Phase 0 (Connections Hub) is SHIPPED** to `alediez2048/main`. Unified `/connections` with all 6 mechanisms + honest verification.

**Phase 1 (Workflows Foundation) is SHIPPED** to `alediez2048/main`, including the Phase 1.5 polish that locked the chat-driven create flow end-to-end (real agent invocation in the drafter, `<workflow-preview>` tag parsing in `AgentMessageBubble`, orchestrator allowlist, channel/webview send stubs).

**Phase 2 + Phase 3 ticket sets are DRAFTED** under `Automations/Tickets/phase-2-execution/` (16 tickets) and `Automations/Tickets/phase-3-canvas/` (10 tickets). Neither started. Phase 2 is the next concrete work; Phase 3 is gated on user demand per `prd.md §5.3`.

**Phase 4 (Browser Agent) ticket set DRAFTED** under `Automations/Tickets/phase-4-browser-agent/` — `F4-overview.md` + 7 sub-tickets (`F4-1` through `F4-7`). Phase 4 is explicitly deferred: do NOT start until Phase 2 and Phase 3 are on `main`. The thesis is a CEF-native CDP-driven browser agent (Stagehand-style `act`/`extract`/`observe` API) that drives the user's already-authenticated webview sessions. Additive to Composio, not a replacement. Read `phase-4-browser-agent/F4-overview.md` for the full architecture + capability gap analysis + reference-repo notes.

---

## What's live on `main` today

### Phase 1 deliverables (F-1 through F-15 + Phase 1.5 polish)

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
- **Channel send + Webview send** — `channel_send` and `webview_account_send` tools are stubs returning "Phase 2 (F2-5) deferral" errors. Workflows touching Channel or Webview connections in their `allowed_connections` won't actually send messages; they'll fail loud with a clear reason (and now also flip the run to `Failed` honestly per F-16 D, instead of lying as `Succeeded` like before). **Composio-routed channels work** (Slack, Discord, Telegram, etc. via Composio's `composio_execute`). Land F2-5 to unify Channel/Webview send.
- **Multi-node chains** — executor rejects `nodes.len() != 1` for Phase 1. F2-2 lands it.
- **Phase 2 trigger types** — webhook, composio_event, channel_message. F2-9/F2-10/F2-11.
- **Phase 2 node kinds** — tool_call, http_request, channel_message, condition, delay. F2-3..F2-7.
- **Hero E2E spec** — `workflows-agent-creation.spec.ts` per F-15's original deliverable. The components + agent invocation are all wired today; the E2E spec authoring is the missing piece.
- **30-day soft-delete + retention sweep** — F-2 hard-deletes today; FR-1.3.4 retention sweep deferred to F2-14.
- **`active_hours` enforcement on cron** — F-7 ignored the field; F2-15.
- **Visual canvas + transform/await/fan_out** — Phase 3.

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

## Phase 2 + Phase 3 ticket sets

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

**Phase 3 — `Automations/Tickets/phase-3-canvas/`** — 10 tickets, ~60h:
- F3-1: @xyflow/react integration + read-only render
- F3-2..F3-3: Palette + per-node config drawer
- F3-4..F3-5: Edge wiring/DAG + live run highlighting
- F3-6..F3-8: transform/await_human_approval/fan_out node kinds
- F3-9: Canvas-driven create flow
- F3-10: Hero E2E + closure

**Phase 4 — `Automations/Tickets/phase-4-browser-agent/`** — overview + 7 sub-tickets, ~23–33 working days for Phase 4.1:
- F4-overview: thesis + 5 architectural forks + capability gap analysis + reference-repo notes
- F4-1: CDP automation primitives (Rust) — 3–5 days
- F4-2: Page perception (DOM + a11y tree grounding) — 3–4 days
- F4-3: LLM-facing tools (browser_observe / browser_act / browser_extract) — 4–6 days
- F4-4: Workflow node integration (`NodeKind::BrowserAction`) — 2–3 days
- F4-5: Live-preview UI surface — 4–5 days
- F4-6: Safety preamble + dry-run + cost caps + audit log — 3–4 days
- F4-7: Vision-grounded fallback (Anthropic computer-use style, opt-in) — 4–6 days

Phase 4.2 (cloud Chromium / Playwright sidecar) is captured in F4-overview's "explicitly deferred" section. Do not start P4 until P2 and P3 are on `main`.

Each phase ships a README index listing open OQs to resolve in the pre-phase brainstorm before starting ticket #1.

---

## Two pre-existing test failures (NOT ours)

These fail under `pnpm test:rust` and predate the branch:

1. **`agent::harness::session::turn::*`** — tests read the developer's real `~/.openhuman/` memory tree instead of an isolated tempdir. Test-isolation bug.
2. **`tools::network::polymarket::place_order_happy_path`** — mock-server contract drift. Passes under plain `cargo test`, fails under the `test:rust` mock wrapper.

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

1. Read this file (`Automations/STATE.md`) to know where the initiative stands.
2. Read `CLAUDE.md` for the repo-level commands + conventions.
3. Read the Phase 1 closure section at the bottom of `Automations/Tickets/phase-1-foundation/DEVLOG.md` for the per-ticket commit table + ADR drift audit.
4. If starting Phase 2: read `Automations/Tickets/phase-2-execution/README.md` and pick a brainstorm OQ to resolve first. Then go through `F2-1.md`.
5. If reporting a Phase 1 bug: re-check this file's "Phase 1.5 polish" + "What's a known deferral" sections before assuming a regression — many "missing" features are documented deferrals.

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
| `Automations/Tickets/phase-{1-foundation,2-execution,3-canvas}/` | Per-phase ticket sets |
| `Automations/ADRs/` | 20 ADRs locked across the initiative |
