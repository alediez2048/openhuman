# Phase 5 — Structured Business Entities + Outcome Observability (PLACEHOLDER)

**Status:** Not drafted. This is a placeholder so the architectural question doesn't get lost as we ship Phases 1.5 → 2 → 3.

**Earliest start:** After Phase 3 ships AND after 3–6 months of real production usage with the F-17 `entity_tags` convention live in the Memory Tree. Schemas emerge from observation, not theory.

---

## Why this phase will eventually exist

OpenHuman's positioning evolved during the 2026-05-22 status grill from "personal AI agent" to "autonomous business-growth agent that runs workflows in the background to grow leads / demos / proposals / payments / traffic / conversions". That positioning requires the system to maintain a coherent view of structured business entities across time:

- *"Show me all leads in 'qualified' stage I haven't contacted in 7 days."*
- *"How many proposals went out in May? How many closed?"*
- *"This Stripe webhook is for Acme Corp — is Acme a customer or a prospect? What was the deal size? Who owns the relationship?"*
- *"My agent sent 50 cold emails last week. How many replied? How many turned into demos? Which sequence converted best?"*

Memory Tree (chunk-based text retrieval) is excellent at "find me what's relevant to this query" but bad at "give me all rows where status='qualified'". The two systems compose; they're not interchangeable. Phase 5 builds the structured layer.

---

## What this phase will likely include

### A. Structured entity tables

Concrete domain models for the entity kinds the F-17 tag convention surfaces. Initial candidates — refine based on what tags actually emerge in production:

- `Lead { id, company, contact_email, stage, source, first_seen_at, last_contacted_at, owner, notes_ref }`
- `Deal { id, lead_id, amount, stage, expected_close_at, actual_close_at, owner }`
- `Proposal { id, deal_id, sent_at, viewed_at, replied_at, status }`
- `Payment { id, source (stripe/...), amount, customer_ref, charged_at, status }`
- `Meeting { id, calendar_event_id, contact_ref, started_at, transcript_ref, outcome_summary }`
- `Customer { id, email, signup_at, plan, mrr, churned_at }`
- `Contact { id, email, full_name, company, role, last_interaction_at }`

Tables live alongside Memory Tree (same SQLite DB, separate schema). Memory chunks carry foreign-key tags pointing into these tables; tables carry `notes_ref` pointing back into Memory Tree chunks for human-readable context.

### B. Entity-aware workflow node kinds

New node kinds that operate on structured entities, not just text:

- `entity_lookup { kind, filter, limit }` — pulls structured rows into the workflow's context.
- `entity_upsert { kind, id, fields }` — inserts or updates a row from a workflow.
- `entity_query { sql_or_jq }` — ad-hoc structured query.

Workflows like "for every lead in stage qualified, send a follow-up via email" become first-class.

### C. Outcome observability — measure agent business impact

The "autonomous agent that grows the business" thesis is unfalsifiable without measurement. Phase 5 ships a metrics layer:

- **Attribution.** When a Stripe payment lands, walk back through Memory Tree + entity tags to find the chain of agent-actions that touched this customer. Did an agent-sent email lead to this signup? Did an agent-scheduled demo convert? Surface attribution percentages per workflow.
- **Per-workflow ROI.** For each enabled workflow: cost (LLM tokens + tool fees + cloud-browser if Phase 3.2) vs outcome ($$$ in conversions attributable). A dashboard surface (extending the Linear-row list to include a "value per run" column when applicable).
- **Channel-source-level conversion funnels.** Where do leads come from? Which channels survive to deal stage? Which workflows are the conversion bottleneck?

### D. Agent prompts that USE the entity layer

`workflow_node` prompt grows a section: "If your goal is about a specific entity (lead, deal, proposal, payment), call `entity_lookup` FIRST to get the latest structured state. Don't reconstruct it from text memory; the structured table is faster and authoritative."

Drafter prompt grows similar awareness so business workflows naturally include `entity_lookup` + `entity_upsert` nodes when appropriate.

---

## What lives in the F-17 hook today

F-17 (Phase 1.5, in progress) ships `WorkflowRunMemory.entity_tags`:

- Runtime auto-tags by connection source (`entity:source:stripe`, `entity:source:gmail`, etc.).
- Agent-authored tags via the `## Entities touched` prompt convention (`entity:lead:acme-corp`, `entity:deal:acme-q3-2026`, etc.).
- No schema validation — free-form `entity:<kind>:<id>` strings.

Phase 5 will read these tags + the chunks they're attached to to bootstrap the structured tables. The 3–6 month run-in period is precisely so we have enough real-world tag data to know which entity kinds matter, which fields they need, and how they relate.

---

## Anti-scope-creep

Phase 5 is NOT:

- A full-blown CRM replacement. OpenHuman remains the agent layer; structured entities are the agent's working memory, not the user's primary record system. Users with HubSpot / Salesforce continue to use those — Phase 5 mirrors or syncs, doesn't replace.
- A business-intelligence platform. Outcome observability is for the user's own visibility into agent-driven impact — not a multi-tenant BI tool.
- A workflow editor. Phase 5 changes node kinds + adds outcome surfaces; it doesn't touch the canvas or chat-drafting paths.

---

## Reading order for the future implementer

1. This placeholder.
2. F-17 (memory loop + entity_tags hook).
3. 3–6 months of `entity_tags` data emerging in real usage.
4. The patterns in (3) tell you which entity kinds to model and which fields they need.
5. Then start drafting Phase 5 properly (overview + sub-tickets in this directory).

Don't skip step 3. Schemas committed without data become migration debt.
