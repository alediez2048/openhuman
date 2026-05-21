# Starter templates

Phase 1 ships four read-only workflow templates bundled into
`openhuman-core` via `include_str!` per ADR-004:

- `ru-1-founder-morning-digest.json`
- `ru-2-linkedin-engagement-queue.json`
- `ru-3-spotify-friday-five.json`
- `ru-4-jira-sprint-retro.json`

The canonical exemplar lives at
`Automations/Artifacts/templates/ru-1-founder-morning-digest.json`;
RU-2..RU-4 follow the same shape.

## File shape

Each file is a JSON document with these top-level fields:

| field | type | notes |
| --- | --- | --- |
| `template_id` | `string` | Stable id (e.g. `"ru-1-founder-morning-digest"`). Catalog dedups against `Workflow.origin = Seed{template_id}`. |
| `min_phase` | `u32` | Minimum Phase needed for the template to run. Phase 1 ships everything with `1`. |
| `name` | `string` | Display name for the catalog card. |
| `description` | `string` | One-sentence subtitle for the catalog card. |
| `tags` | `string[]` | Free-form tags for future filter chips (FR-1.8.6). |
| `trigger` | `Trigger` JSON | Mirrors `workflows::types::Trigger`. Phase 1 supports `cron` + `manual`. |
| `nodes` | array | Each node mirrors `workflows::types::Node`; Phase 1 supports a single `agent_prompt` node per template. |
| `edges` | array | Phase 1 templates have zero edges (single node). |
| `settings` | `WorkflowSettings` JSON | `timeout_secs` + `on_error`. |
| `required_connections` | `ConnectionRef[]` | Union of every `allowed_connections` across nodes. Server uses this for the catalog's `missing_connections` computation. |
| `rationale_at_seed` | `string[]` | Bullet-list rationale shown in the catalog card and on the resulting workflow's preview. |

## Parsing model

`StarterTemplate` parses `trigger` / `nodes` / `edges` / `settings`
as opaque `serde_json::Value` so forward-compat fields (per-node
`name`, per-node `on_error`, future trigger variants) don't reject
the file at parse time. The `raw_payload` returned from
`workflows_list_starter_templates` is the full JSON body and is
passed unchanged to `workflows_create` on [Add].

## Cron expressions

Templates use **standard 5-field crontab** (`min hour dom month
dow`). The `cron` crate the validator (F-11) and scheduler (F-7)
use requires a 6-field expression, so all code paths route through
`crate::openhuman::cron::normalize_expression` which prepends a
`0` seconds field automatically.

## Adding a new template

1. Drop the JSON file under this directory.
2. Add an `include_str!` constant in `mod.rs` and an entry in
   `BUNDLED_JSON`.
3. The `templates_tests` suite will validate parse + cron + unique
   id at build time; CI fails before a malformed template ships.
