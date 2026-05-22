# Phase 3 — Browser Agent

Phase 3 of the Workflows & Automations initiative. Drafted, **not started**.
Phase 2 MUST ship before Phase 3 begins. Phase 4 (canvas editor) is NOT a prerequisite — per the 2026-05-22 Option-B ordering decision, browser-agent is the next priority after Phase 2.

> **Status (2026-05-22):** Phase 3.1 (CEF-native browser agent) is drafted as
> 7 sub-tickets — `F3-1` through `F3-7` — plus the umbrella primer
> [`F3-overview.md`](./F3-overview.md). Phase 3.2 (multi-runtime / cloud
> Chromium follow-up) is captured at the end of `F3-overview.md` and explicitly
> deferred — not in scope for Phase 3.1.

---

## Why Phase 3 exists

OpenHuman today reaches third-party apps through Composio (preferred), Channels
(Slack/Discord/Telegram bot APIs), Webview-account scanners (read-only),
MCP servers, Generic HTTP, and built-in tools. None of these handle:

- **Apps without public APIs** — niche SaaS, banking portals, government sites,
  university systems, internal corporate tools.
- **Multi-step UI flows** — "apply to 5 jobs", "cancel my subscription".
- **Acting under the user's own authenticated session** without granting OAuth
  scopes — "post a tweet from MY account, not via Composio Twitter".

Phase 3 closes that gap with a CDP-driven browser agent that drives the user's
already-authenticated CEF sessions. **Additive to Composio** — Composio remains
the preferred path for everything it supports. Phase 3 is the fallback when
no API path exists or when the user wants UI fidelity.

See [`F3-overview.md`](./F3-overview.md) for the full thesis, capability
comparison, architecture, and the five forks the future implementer must
honor.

---

## Sub-ticket index

| # | Title | Depends on | Est. |
|---|---|---|---|
| [`F3-overview`](./F3-overview.md) | Initiative primer — architecture, forks, capability gaps, anti-scope-creep boundaries. **Read this first.** | — | (doc only) |
| [`F3-1`](./F3-1.md) | CDP automation primitives (Rust) | — | 3–5 days |
| [`F3-2`](./F3-2.md) | Page perception (DOM + a11y-tree grounding) | F3-1 | 3–4 days |
| [`F3-3`](./F3-3.md) | LLM-facing tools — `browser_observe` / `browser_act` / `browser_extract` | F3-1, F3-2 | 4–6 days |
| [`F3-4`](./F3-4.md) | Workflow node integration — `NodeKind::BrowserAction` | F3-3 | 2–3 days |
| [`F3-5`](./F3-5.md) | Live-preview UI surface | F3-3 | 4–5 days |
| [`F3-6`](./F3-6.md) | Safety preamble + dry-run + cost caps + audit log | F3-3, F3-4 | 3–4 days |
| [`F3-7`](./F3-7.md) | Vision-grounded fallback (Anthropic computer-use style) | F3-2, F3-3 | 4–6 days |

**Total Phase 3.1:** 23–33 working days of focused work.

### Reading order

`F3-overview` → `F3-1` → `F3-2` → `F3-3` → `F3-4` → then `F3-5` / `F3-6` /
`F3-7` in parallel.

### What ships at each milestone

- **After F3-3:** power-user-only — workflow hand-edits `allowed_tools` to
  include `browser_*`, can act in CEF.
- **After F3-4:** first-class `BrowserAction` node kind. Drafter can produce
  browser-agent workflows from chat input.
- **After F3-5:** end users see what the agent is doing.
- **After F3-6:** safe to enable for non-power-users. Dry-run default, cost
  caps, audit log, redaction policy.
- **After F3-7:** canvas / SVG / WebGL pages also work via vision fallback.

### Definition of "Phase 3.1 done"

All seven sub-tickets shipped + the end-to-end brokerage-balance acceptance
test in `F3-overview.md` passes against a real site.

---

## Reference repos (analysed in `F3-overview.md`)

| Repo | What to steal |
|---|---|
| [browser-use/browser-use](https://github.com/browser-use/browser-use) | DOM-extraction algorithm — port to Rust |
| [browserbase/stagehand](https://github.com/browserbase/stagehand) | The act/extract/observe three-primitive API shape |
| [vercel-labs/agent-browser](https://github.com/vercel-labs/agent-browser) | Reference for the simplest possible loop |
| [reworkd/AgentGPT](https://github.com/reworkd/AgentGPT) | Autonomy / goal-decomposition patterns (less relevant) |

For Phase 3.2's cloud-Chromium option, [Browserbase Stagehand](https://github.com/browserbase/stagehand)
becomes more central. Phase 3.1 sticks to CEF.

---

## Anti-scope-creep boundaries

Phase 3.1 is NOT:

- A general autonomous agent (one task per invocation, no goal decomposition).
- A replacement for Composio (additive, never replacement).
- A public browser-as-a-service.
- A screen-recording / replay surface.
- A credential vault.
- A CAPTCHA solver.
- Full computer use (browser only — no desktop apps, terminal, or file system).

Phase 3.2 covers:

- Headless Playwright sidecar for "research mode".
- Browserbase / Steel adapter for cloud scale-out.
- Multi-tab orchestration.

See `F3-overview.md` for the full anti-scope-creep section.
