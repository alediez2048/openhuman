# ADR-008: Starter templates are a read-only catalog, not auto-seeded rows

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

Phase 1 ships four curated starter workflows (RU-1..RU-4). The user-experience question: when a user opens `/workflows` for the first time, do they see those templates *already in their workflows list* as paused rows they can toggle on, or do they see them in a separate "browse and add" catalog?

This is more subtle than it sounds. Auto-seeding feels welcoming ("look, here are 4 things you can turn on") but requires watermarking ("don't re-seed if the user deleted them"), introduces a `workspace_state` table, and gives the user rows they didn't ask for. A catalog avoids all of that but requires an extra click to adopt a template. This maps to `requirements.md §8` OQ-12.

## Decision

Starter templates are a **read-only catalog at runtime**, surfaced as a separate "Starter workflows" section on `/workflows`. Templates are not inserted into the user's `workflows` table by any startup process. Each catalog card has [Add to my workflows] and [Add & Enable] buttons; clicking either calls `workflows_create` with the template payload and `origin = Seed { template_id }`, after which the row appears in "Your workflows" and disappears from the catalog (dedup via `origin = Seed { template_id }`). There is no `workspace_state` table and no `workflows_seeded_at_v*` watermark.

## Alternatives considered

**Auto-seed on first `/workflows` visit.** Insert all RU-1..RU-4 templates as `enabled = false` rows the first time the user opens the page. Rejected because the watermark logic gets gnarly fast: what happens when a Phase 2 PR ships RU-5..RU-9? Re-seed only the new ones? What if the user deleted some? Forces a per-version watermark (`workflows_seeded_at_v2 = true` etc.) and a `workspace_state` table that exists *only* for this feature.

**Auto-seed on every launch.** Re-insert any missing templates each app start. Rejected because it doesn't respect user intent — if I delete "Founder morning digest," I don't want it back tomorrow.

**Hybrid (auto-seed once, then catalog).** Auto-seed on first visit, then expose the catalog for templates added in later versions. Rejected because it's the worst of both: still needs the watermark *and* needs the catalog. The catalog alone covers both cases cleanly.

## Consequences

### Positive
- No `workspace_state` table, no migration to add it, no watermark logic to reason about.
- User intent is explicit — every template in "Your workflows" is one the user actively chose. Deletion is permanent in the sense the user expects.
- Phase 2 and beyond can ship new templates by just adding a `.json` file — they appear in the catalog automatically, no migration.

### Negative
- An extra click per template. First-time users see an empty "Your workflows" section above a populated catalog, which is slightly less inviting than rows-already-there.
- We rely on the empty-state CTA ("Ask OpenHuman to build a workflow" + the catalog itself) to carry first-time discoverability — see ADR-001 and FR-1.2.6.

### Neutral
- Dedup is by `template_id` against `origin = Seed { template_id }` — if a user adds a template, deletes it, then re-adds it, the row is recreated with a fresh UUIDv7 but the same `template_id`. Telemetry can distinguish add-then-delete-then-readd from initial-add via `WorkflowDeleted` events.

## Implementation notes

- `workflows_list_starter_templates` RPC reads `include_str!`-bundled JSON, filters by `min_phase <= current_phase`, and excludes templates whose `template_id` is already in the user's `origin = Seed { template_id }` set.
- See `systemsdesign.md §5` for full catalog query pseudocode.
- FR-1.8.1 through FR-1.8.5 in `requirements.md` lock the contract.

## Related ADRs

- ADR-004 (Templates as in-repo JSON) — the storage substrate this decision sits on top of.
- ADR-018 (Workflow origin discriminator) — `Seed { template_id }` is the dedup key.
