# ADR-001: Nav placement — dedicated 8th bottom-tab

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

OpenHuman exposes its top-level surfaces through the bottom tab bar (`BottomTabBar`): Home, Human, Connections, Intelligence, Skills, Chat, etc. Workflows is a new first-class surface — users need to *see* what background automations are running, toggle them on/off, and inspect history without descending into settings.

Where this surface lives determines whether Workflows feels like a primary OpenHuman capability or a hidden power-user feature. This maps to `requirements.md §8` OQ-1.

## Decision

We will add a **dedicated 8th bottom-tab `Workflows`**, positioned between `Connections` and `Intelligence`, with a full-page `/workflows` route. This makes Workflows a first-class peer of Connections and Intelligence — the three surfaces that together describe "what OpenHuman is doing for me."

## Alternatives considered

**A settings sub-tab (`/settings/workflows`).** Cheapest to ship and discoverable to users already in settings. Rejected because workflows are not configuration — they're an *operational* surface (monitoring + activation). Burying them in settings frames them as setup rather than as ongoing daily-use entities, which contradicts the "personal agent that works for you" vision in `prd.md §2`.

**Sidebar-only / chat-thread artifact list.** Surface workflows only inside chat as expandable thread artifacts. Rejected because the user must still be able to find their workflows when they're not actively chatting about them — empty-thread discoverability and the "monitor what's running in the background" use case both die without a top-level entry point.

## Consequences

### Positive
- Workflows is a peer of Connections and Intelligence in the user's mental model.
- One click from anywhere in the app to "what's running in the background."
- Empty-state CTA for "Ask OpenHuman to build a workflow" gets prime real estate.

### Negative
- Bottom tab bar gets denser (8 items). Pushes us closer to the limit of what fits on narrow window widths.
- Adds a new i18n string (`nav.workflows`) and a Heroicon (still OQ-6, see ADR-related notes).

### Neutral
- Tab placement between Connections and Intelligence frames the mental flow "what I'm connected to → what I've automated → what I want to learn/do next." This ordering may shift in later UX research.

## Implementation notes

- `app/src/components/BottomTabBar.tsx` — add `Workflows` entry between `Connections` and `Intelligence`.
- `app/src/lib/i18n/en.ts` — `nav.workflows: 'Workflows'`.
- Route: `app/src/pages/Workflows/WorkflowsList.tsx`, registered in `AppRoutes.tsx`.
- Icon choice (Heroicon) remains OQ-6 — candidates: `BoltIcon`, `ArrowPathRoundedSquareIcon`, `RectangleStackIcon`.

## Related ADRs

- ADR-006 (Connections Hub as Phase 0) — the tab immediately to the left.
- ADR-002 (Phase 1 PR scope) — this ADR is a deliverable inside that scope.
