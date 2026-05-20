# ADR-004: Starter templates shipped as in-repo JSON via `include_str!`

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

Phase 1 ships a curated set of starter workflows (RU-1 through RU-4) for users to add from the `/workflows` catalog. These need to live *somewhere* — bundled with the binary, fetched from a remote backend, or generated at build time.

The decision affects how new templates ship, whether they can update without an app release, and what trust boundary applies to template payloads. This maps to `requirements.md §8` OQ-8.

## Decision

We will ship templates as **in-repo JSON files** at `src/openhuman/workflows/templates/*.json`, bundled into the binary via Rust's `include_str!` macro. `workflows_list_starter_templates` reads them at request time, applies the phase filter and the user-seeded dedup, and returns `StarterTemplateView`s. Phase 1 ships `ru-1-founder-morning-digest.json` through `ru-4-jira-sprint-retro.json`.

## Alternatives considered

**Fetched from a backend service.** Host templates on the OpenHuman backend; the client refreshes a catalog endpoint. Rejected because it introduces a SaaS dependency for what is otherwise a fully local feature — and the catalog is curated and small (4 files Phase 1, ~9 by Phase 2). The cost of "we have to ship an app update to add a template" is negligible against the cost of "the workflows feature now depends on the backend being up."

**Generated at build time from a higher-level DSL.** A build script reads a YAML/TOML description and emits per-template JSON. Rejected because we only have ~9 templates total across all phases; the abstraction would cost more lines than it saves, and there's no use case yet for runtime template synthesis.

## Consequences

### Positive
- Templates are a Rust resource: type-safe at the boundary (the JSON gets validated by `serde` on first load), no network failure modes, no backend dependency.
- Pull-request review of a new template is just a code review of one `.json` file — no separate "template editing" tool.
- Easy to test — unit tests can load each template and run it through the proposal validator to prove every shipped template is structurally valid.

### Negative
- Adding or fixing a template requires an app release. There's no hot-fix path for a broken template.
- Templates aren't customizable per locale at runtime — though `name` and `description` are translatable via i18n keys (NFR-2.7.3 marks Phase 1 English-only with `// translate later`).

### Neutral
- If future demand for community templates emerges, this decision is reversible — `workflows_list_starter_templates` can switch to a backend-fetched source without touching the catalog UI.

## Implementation notes

- Templates at `src/openhuman/workflows/templates/*.json`, embedded by `include_str!` macro in `workflows/ops.rs` or a sibling module.
- Each template JSON shape: `{ template_id, min_phase, name, description, trigger, nodes, edges, settings, required_connections }` — see `systemsdesign.md §5.1`.
- Pseudocode for `list_starter_templates` in `systemsdesign.md §5.2`.

## Related ADRs

- ADR-008 (Templates as read-only catalog) — defines the runtime contract this storage decision supports.
- ADR-018 (Workflow origin discriminator) — `origin = Seed { template_id }` is the dedup key used against this in-repo set.
