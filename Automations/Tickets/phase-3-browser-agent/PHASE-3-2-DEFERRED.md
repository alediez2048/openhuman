# Phase 3.2 — deferred work from Phase 3.1

Phase 3.1 shipped the entire backend surface of the Browser Agent — F3-1 through F3-4, F3-4.5, F3-5 chunks 1+2a, F3-6 chunks 1+2+3+4a. The hero use case ("post on LinkedIn via browser automation") is technically operable end-to-end on the fork. What's deferred:

## F3-5 chunk 2b — React `BrowserPreviewPanel`

Rust foundation is in place (broadcaster captures screenshots on action boundaries, socket bridge republishes as `browser_preview_frame` WebChannelEvent). The frontend consumer is the only missing piece.

Components per the F3-5 ticket:
- `BrowserPreviewPanel` — container, subscribes to socket events filtered by `event === 'browser_preview_frame'` + `run_id`.
- `BrowserScreenshot` — `<img>` with base64 PNG src + bounding-box overlay.
- `BrowserActionLog` — virtualised scrolling list of every `browser_*` tool call (read from the existing audit-log RPC, which needs to be added at the same time).
- `BrowserTakeoverButton` — pauses agent + surfaces the CEF window for direct user control (needs Tauri window-mgmt plumbing).
- `BrowserDryRunToggle` — set at workflow-create time only, read-only mid-run.

Estimated: ~3-4 days frontend + ~1 day Rust (add `browser_agent_get_audit_log` RPC controller).

## F3-6 chunk 4b — screenshot pixel redaction

Black-bar overlay on bounding boxes of password / SSN / card-number input fields. Applied to broadcast frames AND persisted screenshot artifacts.

Needs:
- `image` crate dependency.
- Bounds-aware compositing pipeline (read Snapshot.elements, find redaction targets, draw black rect on PNG).
- Integration with `capture_and_broadcast` so the broadcast frame is post-redaction.
- Integration with the (future) screenshot persistence path so disk artifacts are post-redaction.

Estimated: ~1 day Rust.

## F3-6 chunk 4c — per-action confirmation gate

`ConfirmationPolicy` evaluator + tool-level gate. When triggered, `browser_act` returns `{ status: "requires_confirmation", action, screenshot, timeout: 120s }` and waits for a `DomainEvent::BrowserActionConfirmation { run_id, confirmed: bool }`.

Default policy (per F3-6 ticket):
- Form submits on URL paths containing `signin|login|register|password|billing|payment|checkout|account|profile|settings` → confirmation.
- Any write action on a non-trusted host → confirmation.
- User-managed trusted-hosts list in prefs.

Depends on F3-5 chunk 2b for the user-facing Confirm/Reject UI. Estimated: ~2 days Rust + frontend integration.

## F3-7 — vision-grounded fallback

Opt-in. When DOM-grounding fails (e.g. canvas-heavy app, custom rendering), fall back to Anthropic computer-use-style vision tool that operates on screenshots + raw coordinates instead of element ids.

Estimated: ~4-6 days. Separate vision-tier LLM cost surface. Worth its own ticket review before starting.

## Webview warmth (cron path)

Today the opener auto-creates a tab from stored cookies when no live tab exists, which covers manual-run cases (user clicks Run Now, opener spins up a tab). For cron-triggered runs at 8am with no human present, the same code path works as long as the user-data-dir cookies are valid. If cookies expire, the safety preamble's `session_expired` path catches it and the run fails cleanly.

A future enhancement could:
- Periodically refresh authenticated sessions (warm pings to provider home URLs from a background task).
- Surface session-expiry warnings in the UI before they bite a scheduled run.

Not blocking; the runtime behaves correctly today. Estimated: ~2-3 days when prioritised.

## Drafter dry-run-by-default

F3-6 chunk 2 follow-up — `workflow_propose_create` should default new `browser_action` workflows to `dry_run: true` as training wheels. The user then edits to `false` after verifying. Today the drafter takes the user's explicit cue ("dry-run mode first so I can verify") OR defaults to `false`.

Estimated: ~1 hour. Single prompt change in `workflow_builder.md` plus a regression test.

---

## Notes on prioritisation

The biggest user-facing gap is F3-5 chunk 2b (the React panel). Without it, users running a browser_action workflow can't watch what the agent is doing in real time — they only see the final status + audit log post-run. The trust UX deficit is real, but the backend pipe is verified working today; this is purely a frontend project.

After F3-5 chunk 2b:
- F3-6 chunks 4b and 4c become unblocked.
- F3-7 is opt-in and can be sequenced independently.

Webview-warmth and drafter-default are quick wins (1 day each) and can land any time.
