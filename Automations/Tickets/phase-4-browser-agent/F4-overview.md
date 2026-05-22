# F4 Initiative Primer — Browser Agent Capabilities

**For:** Future coding-agent session(s)
**Project:** OpenHuman — Workflows & Automations
**Date drafted:** 2026-05-22
**Dependencies:** Phase 2 (multi-node workflows, F2-1 → F2-16) and Phase 3 (canvas editor, F3-1 → F3-10) MUST be shipped first. Do not start F4 work until both are on `main`.
**Estimated total scope (Phase 4.1):** 3–6 weeks of focused work across 7 sub-tickets (`F4-1` through `F4-7`).
**Estimated total scope (Phase 4.2 follow-up, multi-runtime):** +2–3 weeks. Not covered by this primer in detail — see "Phase 4.2 — explicitly deferred" section.

---

## What Is This Initiative?

Build a **CDP-driven browser agent** on top of OpenHuman's existing CEF (Chromium Embedded Framework) child-webview infrastructure. The agent observes a page via DOM + accessibility-tree grounding, takes actions via CDP (`Input.dispatchMouseEvent`, `Input.insertText`, `Page.navigate`, etc.), and exposes a tight three-primitive surface to the LLM: `browser_observe`, `browser_act`, `browser_extract` (modeled on Stagehand's API).

The browser is the user's **already-authenticated CEF session**. No separate Chromium process for Phase 4.1, no cloud browser, no JS injection (per CLAUDE.md's standing constraint). The agent drives the user's real logged-in tabs.

### Why It Matters

OpenHuman today reaches a third-party app through one of these mechanisms:

1. **Composio** — OAuth-grant programmatic access. Best when available.
2. **Channels (Phase 1)** — Slack/Discord/Telegram bot APIs.
3. **Webview-account scanners (Phase 1)** — per-provider CDP-driven READ-ONLY scrapers.
4. **MCP servers / Generic HTTP / Built-in tools** — narrow targeted use.

None of these handle the **long tail of apps without good APIs**, **multi-step UI flows**, or **acting under the user's own authenticated session without granting OAuth scopes**. The 2026-05-22 brainstorm with the user established the capability gap explicitly. F4 closes it.

What F4 unlocks (concrete examples):

- *"Every Monday, log into my brokerage and email me my portfolio balance"* — no API exists.
- *"Apply to the 5 newest jobs on my saved LinkedIn search, customizing the cover letter per role"* — multi-step UI flow.
- *"Cancel my Spotify subscription"* — no Composio coverage for that action.
- *"Reply to every Twitter @mention in my voice, under my own account, without granting Composio Twitter write-scopes"* — authenticated browser session.
- *"Find the CTO and head-of-engineering for every YC W24 batch company"* — multi-site research + extraction.

What F4 explicitly does NOT replace:

- Composio-supported actions stay on Composio. Browser-driving is 10–100× slower per action than `composio_execute`. F4 is **additive to** the existing stack, not a replacement.
- Generic web search stays on `web_search_tool`. F4 is for *multi-step* navigation, not single-query lookups.
- The Phase 1 per-provider scanners (Gmail / Slack / Telegram / Discord / LinkedIn) stay as-is — they're more efficient than a general browser agent for their narrow use cases. F4 is the **fallback** when those scanners don't cover the user's specific need.

### Decision-tree the workflow-drafter / orchestrator should follow (post-F4):

1. Is there a **Composio** action for what the user described? Use it.
2. No? Is there a **Channel** integration? Use it.
3. No to both? Is there a **per-provider scanner** that does what we need? Use it.
4. No? **Browser agent (F4)** drives the UI under the user's authenticated session.

The drafter prompt + the workflow_node prompt MUST encode this priority order so the browser agent isn't reached for when a faster path exists. This is the same family of decision as F-16's "don't delegate when you have the direct tool".

---

## What Was Already Done (Phase 0 → Phase 3 prerequisites)

By the time F4 starts, the following must be on `main`:

- **Phase 0** — Connections Hub at `/connections`. ✅ shipped.
- **Phase 1** — Workflows Foundation (F-1 → F-16). ✅ shipped (F-16 closed 2026-05-22).
- **Phase 2** — Execution Depth (F2-1 → F2-16). Multi-node chains, real Channel + Webview outbound send, retries, scheduler. **Required** because a browser action is one node in a chain (read with Composio Gmail → react with browser-agent on LinkedIn UI → send summary via Composio Slack).
- **Phase 3** — Canvas Editor (F3-1 → F3-10). Visual workflow builder. **Required** because the browser-agent node needs UI affordances (action description input, dry-run preview, domain whitelist) the canvas editor will surface.

### Existing infrastructure F4 can build on

- `app/src-tauri/src/webview_accounts/` — CEF child-webview lifecycle, per-user profile isolation, authenticated session persistence.
- `app/src-tauri/src/*_scanner/` — per-provider modules already drive CDP from Rust (Telegram-web, WhatsApp-web, Slack, Discord, LinkedIn, etc.). F4 generalizes the CDP wrapper.
- `src/openhuman/agent/agents/workflow_node/` (F-16) — constrained sub-agent identity with per-instance `allowed_tools` override. The `browser_*` tools will be added to its allowlist for workflows that opt in.
- `src/openhuman/connections/types.rs::ConnectionRef::Webview` — already exists; F4 adds a new variant or repurposes Webview to carry a "browser-agent-capable" flag.
- `src/openhuman/tools/impl/browser/native_backend.rs` — existing browser tooling for Composio-routed browser actions. F4 generalizes / extends this.

### What's notably absent (and F4 must build)

- A generic, non-provider-specific CDP automation layer in Rust.
- A page-grounding primitive (DOM-tree + accessibility-tree extraction → LLM-readable structured summary).
- A `browser_act` tool that translates natural-language commands into CDP calls.
- Safety preambles, dry-run mode, per-action confirmation thresholds for the browser agent.
- A live-preview UI surface so the user can watch the agent work + take over.

---

## Architecture — Direction A (Phase 4.1, recommended)

**CEF-native, user-supervised, single agent identity.**

```text
┌──────────────────────────────────────────────────────────────────┐
│  workflow_node sub-agent (existing F-16 archetype)              │
│  with browser_* tools added to allowed_tools                    │
└──────────────────────────────────────────────────────────────────┘
                  ↓ LLM emits browser_act("click Save")
┌──────────────────────────────────────────────────────────────────┐
│  browser_act / browser_observe / browser_extract tools          │
│  (`src/openhuman/tools/impl/browser_agent/`)                    │
└──────────────────────────────────────────────────────────────────┘
                  ↓ resolved to CDP primitive calls
┌──────────────────────────────────────────────────────────────────┐
│  Page perception layer (`src/openhuman/browser_agent/perceive/`)│
│  DOM tree + accessibility tree → structured snapshot            │
└──────────────────────────────────────────────────────────────────┘
                  ↓ + ↑
┌──────────────────────────────────────────────────────────────────┐
│  CDP automation primitives (`src/openhuman/browser_agent/cdp/`)  │
│  click, type, scroll, navigate, screenshot, wait                 │
└──────────────────────────────────────────────────────────────────┘
                  ↓ chrome-devtools-protocol
┌──────────────────────────────────────────────────────────────────┐
│  CEF child webview (existing webview_accounts infrastructure)    │
│  Authenticated session, per-user profile                         │
└──────────────────────────────────────────────────────────────────┘
```

### The five forks captured (with recommended answers)

| Fork | Options | Recommendation for Phase 4.1 |
|---|---|---|
| **Runtime location** | CEF / headless Playwright / cloud (Browserbase) | **CEF** — reuses authenticated sessions, integrates with the existing scanner infrastructure, no JS injection needed. Other runtimes are Phase 4.2. |
| **Grounding strategy** | DOM-first / vision-first / hybrid | **DOM-first with accessibility-tree augmentation; vision is opt-in fallback (F4-7) for ambiguous cases.** DOM-first is dramatically cheaper in tokens. Vision is opt-in not opt-out — guards token costs. |
| **LLM-facing API** | act/extract/observe (Stagehand-style) vs low-level click/type (computer-use-style) | **act/extract/observe.** Smaller surface, easier to constrain in safety preamble, lets the LLM stay at "intent" level. Low-level primitives are available internally for the tool impls but NOT exposed to the LLM. |
| **Workflow integration** | New node kind + new chat tool | **Both.** `NodeKind::BrowserAction` for scheduled/unattended; `browser_act`/`browser_observe`/`browser_extract` for ad-hoc chat use (gated by feature flag for the chat surface in Phase 4.1; opens up in Phase 4.2). |
| **Safety model** | Live preview / dry-run / per-action confirmation / domain whitelist | **All four, layered.** Live preview by default; dry-run for first run of any workflow; per-action confirmation auto-enabled on financial/legal/government domains via whitelist; cost cap as hard ceiling. Detail in F4-6. |

### Why not Direction B (hybrid Playwright) for Phase 4.1?

- Phase 4.1's thesis is "act under the user's authenticated session" — the differentiating capability. Playwright as a separate runtime can't easily inherit the user's logged-in CEF session without copy-paste cookie hacks that get fragile.
- Headless Chromium adds a second process to manage (lifecycle, crashes, memory). The CEF already-running webview is the cheaper substrate.
- Generic web research (where Playwright would shine) is the **second-most-important** F4 use case, not the first. Defer to Phase 4.2.

If Phase 4.2 demand materializes (user asks for "research-mode" workflows that don't need authenticated sessions), add a `BrowserRuntime` enum + per-task runtime selection. The architecture above is forward-compatible — the CDP primitives layer is the same shape regardless of which Chrome you're driving.

### Why not Direction C (Browserbase) for Phase 4.1?

- Cost: ~$0.10–0.50 per session. At any meaningful scale, that's a Composio-comparable expense for a strictly inferior capability.
- Authenticated sessions only work via Browserbase's "context persistence" feature, which has known quirks and requires uploading session state (privacy concern for the OpenHuman thesis).
- Adding a paid-third-party dependency to the workflow runtime contradicts OpenHuman's "your real authenticated browser" differentiation.
- If commercial cloud-browser becomes worth it (large-scale headless research), it's a Phase 4.2+ option — explicitly NOT a Phase 4.1 path.

---

## Sub-ticket breakdown (Phase 4.1)

Seven sub-tickets, sized for a single coding-agent session each. They build on each other in order; later tickets assume earlier ones have landed.

| Ticket | Title | Depends on | Est. |
|---|---|---|---|
| [`F4-1`](./F4-1.md) | CDP automation primitives (Rust core) | — | 3–5 days |
| [`F4-2`](./F4-2.md) | Page perception (DOM + a11y tree grounding) | F4-1 | 3–4 days |
| [`F4-3`](./F4-3.md) | LLM-facing tools (`browser_observe`/`act`/`extract`) | F4-1, F4-2 | 4–6 days |
| [`F4-4`](./F4-4.md) | Workflow node integration (`NodeKind::BrowserAction`) | F4-3 | 2–3 days |
| [`F4-5`](./F4-5.md) | Live-preview UI surface | F4-3 | 4–5 days |
| [`F4-6`](./F4-6.md) | Safety preamble + dry-run + cost caps + audit log | F4-3 | 3–4 days |
| [`F4-7`](./F4-7.md) | Vision-grounded fallback (Anthropic computer-use style) | F4-2, F4-3 | 4–6 days |

Total: **23–33 working days** for Phase 4.1.

### What ships at each milestone

- After **F4-3**: an OpenHuman power user can call `browser_observe` / `browser_act` / `browser_extract` from a workflow's `agent_prompt` node. No UI, no safety guards — internal-use only. Validates the core primitives work.
- After **F4-4**: workflows can declare a first-class `BrowserAction` node alongside `agent_prompt`. Multi-node workflows mixing Composio + browser become possible.
- After **F4-5**: end-users watch the agent work in a side panel. Safety + observability story is intact.
- After **F4-6**: safe to enable for non-power-users. Dry-run mode + cost caps + domain whitelist + audit log.
- After **F4-7**: the agent works on canvas/SVG-heavy apps (Figma-style UIs, some banking sites) where DOM-only grounding fails.

---

## Reference repos — what to steal from each

| Repo | Take | Skip |
|---|---|---|
| [`browser-use/browser-use`](https://github.com/browser-use/browser-use) | DOM-tree extraction algorithm (which elements are "actionable"), element-numbering scheme for LLM addressing, retry-on-stale-element patterns. | Their Python/Playwright stack — we'd port the ideas to Rust + CDP. |
| [`browserbase/stagehand`](https://github.com/browserbase/stagehand) | The `act()`/`extract()`/`observe()` three-primitive API shape. The natural-language-to-DOM-action translator pattern. Their schema for `extract()` (Zod-style structured extraction). | Their Browserbase coupling — we run on CEF, not cloud Chromium. |
| [`vercel-labs/agent-browser`](https://github.com/vercel-labs/agent-browser) | Reference implementation of the simplest possible loop (observe → reason → act → repeat). | Light on architecture; mainly useful as a hello-world for the loop shape. |
| [`reworkd/AgentGPT`](https://github.com/reworkd/AgentGPT) | Autonomy/goal-decomposition patterns (less relevant — our agent identity is per-task, not autonomous-goal). | Most of it — different problem space (autonomous goal pursuit vs scoped browser action). |

For F4-1 implementation, the primary reference is **browser-use** (DOM-extraction logic). For F4-3 (LLM-facing tools), the primary reference is **Stagehand** (API shape). For F4-7 (vision fallback), the primary reference is **Anthropic's computer-use docs** — not the listed repos.

---

## Phase 4.2 — explicitly deferred

Not part of Phase 4.1; do NOT scope-creep into F4-1..F4-7. Phase 4.2 is its own initiative:

- **Headless Playwright sidecar** for "research-mode" / non-authenticated browsing. Separate process, separate lifecycle.
- **Runtime selector** — agent picks CEF (user session) vs Playwright (anonymous) per task.
- **Browserbase / Steel adapter** as an optional cloud-Chromium target for scale-out.
- **Multi-tab orchestration** — agent opens 5 tabs in parallel for research.
- **Per-domain selector profiles** — pre-trained DOM patterns for common sites (Twitter / LinkedIn / etc.) so the LLM doesn't re-discover them every run.

Phase 4.2 lands once Phase 4.1 has 6+ weeks of production usage and a clear demand signal for the additional capabilities. Don't pre-build it.

---

## Risks + open questions

### Known risks (call out before starting)

1. **Anti-bot detection.** Cloudflare/PerimeterX/etc. fingerprint CDP-driven sessions and serve CAPTCHAs. Phase 4.1 doesn't solve this — we punt with a clear "site appears to block automation" error and ask the user to take over. Mitigation longer-term: humanized typing rhythms, mouse-movement noise, user-agent rotation. Not Phase 4.1's job.
2. **2FA / login redirects.** When a session expires mid-run, the agent must detect the login wall and ask the user to re-auth — never try to type credentials. F4-6's safety preamble enforces this.
3. **Selector brittleness.** UIs change. The DOM-tree grounding will break when sites redesign. Mitigation: snapshot-then-act, with the LLM re-snapshotting after each failed action. NOT pre-cached selectors.
4. **Cost explosion.** Vision grounding is 10–100× more expensive than DOM. F4-7 must gate vision behind explicit fallback triggers + a hard per-session cost cap.
5. **Privacy.** The browser agent sees the user's logged-in pages. Screenshots may capture PII. F4-6's audit log must redact / hash sensitive fields; the live-preview UI must not auto-record to disk.

### Open questions (defer to F4-1 implementation kickoff)

- Does the existing CEF child-webview lifecycle support a "hidden" mode for unattended workflow runs (cron at 8am, user not present)? If not, headless Chromium becomes a hard dependency earlier than Phase 4.2.
- What's the right unit of "browser session" for an agent? Per-workflow-run? Per-thread? Per-user? Defaults probably per-workflow-run for isolation.
- How does this interact with the user's CEF cookies if they're using OpenHuman simultaneously in another tab? Lock the agent's session to a separate profile or share?

These get answered as part of F4-1's design work, not deferred indefinitely.

---

## "What this initiative IS NOT" — anti-scope creep

- **Not a general autonomous agent.** F4's browser agent does ONE task per invocation. No goal decomposition; no recursive planning; no multi-day autonomous pursuit. That's a different problem (closer to AgentGPT's space).
- **Not a replacement for Composio.** Composio remains the preferred path for everything it supports.
- **Not a public-facing browser-as-a-service.** OpenHuman drives the user's own browser, period. No "API for other apps to use OpenHuman's browser".
- **Not a screen-recording surface.** Live preview during the run, yes. Post-hoc recording / replay: not Phase 4.1.
- **Not a credential vault.** OpenHuman never types passwords or 2FA codes. The user is already logged in OR the agent stops and asks.
- **Not a CAPTCHA solver.** When the agent hits a CAPTCHA, it stops + asks the user.
- **Not full computer use.** Browser only. No desktop apps, no terminal, no file-system manipulation outside the browser context.

---

## Definition of "Phase 4.1 done"

All seven sub-tickets shipped, each with passing unit tests + integration tests. Plus the following end-to-end acceptance:

1. A user can author a workflow in chat: *"Every Monday at 9am, log into my brokerage at example.com, navigate to my portfolio page, extract the total balance, and Slack me the number"*.
2. Drafter produces a 2-node proposal: node 1 = `BrowserAction` (login + navigate + extract), node 2 = `agent_prompt` with Composio Slack send.
3. User clicks Save & Enable.
4. The cron fires Monday 9am. The CEF browser opens (hidden by default; user can opt to see it). The agent navigates, extracts the balance, returns the number.
5. Node 2 receives the number, sends the Slack DM.
6. Run history shows: 2 steps Succeeded, screenshot of the brokerage page captured at extract time, audit log of every CDP action.
7. If the brokerage's login session has expired, the agent stops + the workflow run is marked `Failed { reason = "session expired; user must re-authenticate" }`. The user is notified.

If all seven points work end-to-end on a real brokerage / portal site, Phase 4.1 is shipped.

---

## Reading order for the implementing agent

1. This primer (F4-overview).
2. `F4-1.md` (CDP primitives) — start here.
3. `F4-2.md` (page perception) — depends on F4-1.
4. `F4-3.md` (LLM tools) — depends on F4-1 + F4-2.
5. `F4-4.md` (workflow node) — depends on F4-3.
6. `F4-5.md`, `F4-6.md`, `F4-7.md` in parallel after F4-4 — independent of each other.

When in doubt about a tradeoff not covered in a sub-ticket, default back to this primer's "five forks" recommendations. If those don't answer, ASK the user before committing.
