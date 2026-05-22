# Phase 4 — Visual Canvas (Deferred)

Power-user surface for inspecting and tweaking workflows visually.
**Not required for the core product story** — agent + chat already
cover creation and editing.

> **Status:** Drafted — deferred per `prd.md §5.3`. Revisit only if
> users repeatedly ask for it after Phase 1 + Phase 2 land. This
> ticket set exists so a Phase 4 kickoff doesn't start from a blank
> slate.

---

## Scope (locked by `prd.md` + `requirements.md §1.7`)

The user can:

- **See every workflow as a node graph** at `/workflows/<id>/canvas`
  with @xyflow/react.
- **Drag node kinds from a sidebar palette** onto the canvas to
  build new workflows visually (alternative to the chat-driven path).
- **Edit node config in a side drawer** that opens on click.
- **Wire edges between nodes** including branching (full DAG, not
  just linear chain) — the same validator + executor already
  support it from Phase 2.
- **Watch a run light up the graph** in real time as each node
  fires.
- **Use Phase 4 node kinds**: `transform` (JSON shaping between
  steps), `await_human_approval` (pauses until the user clicks
  Approve), `fan_out` (run children in parallel).

---

## The Phase 4 tickets

| Ticket | Title |
|---|---|
| F4-1 | Canvas scaffold — @xyflow/react integration + `/workflows/<id>/canvas` read-only render |
| F4-2 | Node palette sidebar with drag-source for every NodeKind |
| F4-3 | Per-node config drawer (click-to-edit on the canvas) |
| F4-4 | Edge wiring + DAG validation (drop the Phase 2 linear-chain assumption) |
| F4-5 | Live run highlighting via `WorkflowRunStepStarted` / `WorkflowRunStepCompleted` events |
| F4-6 | `transform` node kind (JSONPath / jq-style shaping) |
| F4-7 | `await_human_approval` node kind + approval UI surface |
| F4-8 | `fan_out` node kind + parallel-children executor |
| F4-9 | Canvas-driven create flow — "New workflow" button opens blank canvas |
| F4-10 | Phase 4 hero E2E (build a fan-out workflow via canvas) + DEVLOG closure + ADR drift audit |

---

## Pre-Phase-3 brainstorm

Decide before starting F4-1:

| OQ | Question | Lean |
|---|---|---|
| OQ-15 | Canvas library — @xyflow/react vs Reaflow vs build-own? | @xyflow/react (locked in `techstack.md`) |
| OQ-16 | Per-node config drawer — modal vs sidebar? | Sidebar that pushes the canvas left |
| OQ-17 | Run highlighting style — pulse vs glow vs colored border? | Colored border + step-time tooltip |
| OQ-18 | Fan-out concurrency cap? | 5 parallel children default; per-node override up to 20 |
| OQ-19 | `await_human_approval` notification — toast / OS notification / both? | OS notification + in-app banner; user can configure |
| OQ-20 | Canvas-driven create — keep chat flow as primary? | Yes; canvas is the "alternative" tab inside the same `/workflows/new` route |

---

## Pre-existing dependencies

- **Phase 2 executor + validator** already support DAGs via
  `topological_sort` (F2-2) and routing decisions (F2-6).
- **Reserved node kinds** `transform`, `await_human_approval`,
  `fan_out` were declared in F-1's `NodeKind` enum from day 1.
- **Event bus** publishes `WorkflowRunStepStarted` /
  `WorkflowRunStepCompleted` — Phase 4 just subscribes in the
  canvas component for live highlighting.

---

## What Phase 4 deliberately does NOT do

- Doesn't replace the chat-driven creation path. Chat stays the
  hero per ADR-007.
- No multi-user collaboration on a canvas.
- No undo/redo beyond browser back / forward (handled by route
  state).
- No canvas zoom-out "all workflows" view — that's the existing
  `/workflows` list page.

---

## After Phase 4

- **Phase 4** (deeper external-platform integrations) — gated on
  adoption metrics per `prd.md §5.4`.
- Possible Phase 5: shared workflows / templates marketplace.
  Tracked as a non-goal in `prd.md §7` for now.
