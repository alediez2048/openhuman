# Phase 4 — Browser Agent

Phase 4 of the Workflows & Automations initiative. Drafted, **not started**.
Phase 2 and Phase 3 must ship before Phase 4 begins.

> **Status (2026-05-22):** Phase 4.1 (CEF-native browser agent) is drafted as
> 7 sub-tickets — `F4-1` through `F4-7` — plus the umbrella primer
> [`F4-overview.md`](./F4-overview.md). Phase 4.2 (multi-runtime / cloud
> Chromium follow-up) is captured at the end of `F4-overview.md` and explicitly
> deferred — not in scope for Phase 4.1.

---

## Why Phase 4 exists

OpenHuman today reaches third-party apps through Composio (preferred), Channels
(Slack/Discord/Telegram bot APIs), Webview-account scanners (read-only),
MCP servers, Generic HTTP, and built-in tools. None of these handle:

- **Apps without public APIs** — niche SaaS, banking portals, government sites,
  university systems, internal corporate tools.
- **Multi-step UI flows** — "apply to 5 jobs", "cancel my subscription".
- **Acting under the user's own authenticated session** without granting OAuth
  scopes — "post a tweet from MY account, not via Composio Twitter".

Phase 4 closes that gap with a CDP-driven browser agent that drives the user's
already-authenticated CEF sessions. **Additive to Composio** — Composio remains
the preferred path for everything it supports. Phase 4 is the fallback when
no API path exists or when the user wants UI fidelity.

See [`F4-overview.md`](./F4-overview.md) for the full thesis, capability
comparison, architecture, and the five forks the future implementer must
honor.

---

## Sub-ticket index

| # | Title | Depends on | Est. |
|---|---|---|---|
| [`F4-overview`](./F4-overview.md) | Initiative primer — architecture, forks, capability gaps, anti-scope-creep boundaries. **Read this first.** | — | (doc only) |
| [`F4-1`](./F4-1.md) | CDP automation primitives (Rust) | — | 3–5 days |
| [`F4-2`](./F4-2.md) | Page perception (DOM + a11y-tree grounding) | F4-1 | 3–4 days |
| [`F4-3`](./F4-3.md) | LLM-facing tools — `browser_observe` / `browser_act` / `browser_extract` | F4-1, F4-2 | 4–6 days |
| [`F4-4`](./F4-4.md) | Workflow node integration — `NodeKind::BrowserAction` | F4-3 | 2–3 days |
| [`F4-5`](./F4-5.md) | Live-preview UI surface | F4-3 | 4–5 days |
| [`F4-6`](./F4-6.md) | Safety preamble + dry-run + cost caps + audit log | F4-3, F4-4 | 3–4 days |
| [`F4-7`](./F4-7.md) | Vision-grounded fallback (Anthropic computer-use style) | F4-2, F4-3 | 4–6 days |

**Total Phase 4.1:** 23–33 working days of focused work.

### Reading order

`F4-overview` → `F4-1` → `F4-2` → `F4-3` → `F4-4` → then `F4-5` / `F4-6` /
`F4-7` in parallel.

### What ships at each milestone

- **After F4-3:** power-user-only — workflow hand-edits `allowed_tools` to
  include `browser_*`, can act in CEF.
- **After F4-4:** first-class `BrowserAction` node kind. Drafter can produce
  browser-agent workflows from chat input.
- **After F4-5:** end users see what the agent is doing.
- **After F4-6:** safe to enable for non-power-users. Dry-run default, cost
  caps, audit log, redaction policy.
- **After F4-7:** canvas / SVG / WebGL pages also work via vision fallback.

### Definition of "Phase 4.1 done"

All seven sub-tickets shipped + the end-to-end brokerage-balance acceptance
test in `F4-overview.md` passes against a real site.

---

## Reference repos (analysed in `F4-overview.md`)

| Repo | What to steal |
|---|---|
| [browser-use/browser-use](https://github.com/browser-use/browser-use) | DOM-extraction algorithm — port to Rust |
| [browserbase/stagehand](https://github.com/browserbase/stagehand) | The act/extract/observe three-primitive API shape |
| [vercel-labs/agent-browser](https://github.com/vercel-labs/agent-browser) | Reference for the simplest possible loop |
| [reworkd/AgentGPT](https://github.com/reworkd/AgentGPT) | Autonomy / goal-decomposition patterns (less relevant) |

For Phase 4.2's cloud-Chromium option, [Browserbase Stagehand](https://github.com/browserbase/stagehand)
becomes more central. Phase 4.1 sticks to CEF.

---

## Anti-scope-creep boundaries

Phase 4.1 is NOT:

- A general autonomous agent (one task per invocation, no goal decomposition).
- A replacement for Composio (additive, never replacement).
- A public browser-as-a-service.
- A screen-recording / replay surface.
- A credential vault.
- A CAPTCHA solver.
- Full computer use (browser only — no desktop apps, terminal, or file system).

Phase 4.2 covers:

- Headless Playwright sidecar for "research mode".
- Browserbase / Steel adapter for cloud scale-out.
- Multi-tab orchestration.

See `F4-overview.md` for the full anti-scope-creep section.
