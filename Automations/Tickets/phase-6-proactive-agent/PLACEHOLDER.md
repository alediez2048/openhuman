# Phase 6 — Proactive Agent (Agent-Initiated Workflows) — PLACEHOLDER

**Status:** Not drafted. This is a placeholder so the long-term product north-star doesn't get lost as we ship Phases 1.5 → 2 → 3 → 5.

**Earliest start:** After Phase 5 (structured business entities + outcome observability) has been live for 2–4 months. Phase 6 reads Phase 5's structured tables + Memory Tree pattern data, so it cannot start before both exist.

---

## The thesis (confirmed in the 2026-05-22 status grill)

OpenHuman's endpoint is **an AI employee that notices and acts**, not just an automation platform that fires on triggers.

User's framing (verbatim): *"The more workflows I build, the more chats, the more integrations, the more OpenHuman learns from me and this kickstarts a proposal feature where OpenHuman gives me proactive proposals based on its memory and pattern recognition."*

That's the **compounding loop**:

```text
more workflows + chats + integrations
        ↓
more data in Memory Tree + structured entities (Phase 5)
        ↓
better pattern recognition by Phase 6's proactive agent
        ↓
better proposals → user approves more → more workflows
        ↓
(loop)
```

Without Phase 6, OpenHuman is a powerful automation platform users CONFIGURE. With Phase 6, OpenHuman becomes an AI employee that **shows up to work with ideas**.

---

## Concrete examples of agent-initiated proposals

These are NOT cron-triggered. They are NOT webhook-triggered. They emerge from continuous pattern scanning over Memory Tree + Phase-5 structured entities:

- **Stalled lead.** "Three leads in your CRM haven't been contacted in 7+ days, all originally tagged 'qualified'. Drafted follow-up emails. Approve to send."
- **Proposal silent.** "Bob from Acme opened your proposal 4 days ago and hasn't replied. Drafted a low-pressure nudge."
- **Calendar opportunity.** "A 90-min slot opened up tomorrow (cancellation). You have two warm leads who asked for time this week. Drafted invite to the highest-priority one."
- **Pattern recognition.** "You've manually scheduled a 1:1 with each new customer for 3 weeks running. Want me to add this as a workflow that fires on `composio_event: Stripe subscription created`?"
- **Anomaly.** "Revenue tracker shows MRR dropped 12% week-over-week — biggest weekly drop in 4 months. Drafted a 'reach out to recent churners' workflow."
- **Content opportunity.** "Your top-of-funnel post traffic peaked Mondays for the last 6 weeks. You haven't published anything for next Monday. Drafted three post ideas based on recent customer questions."
- **Routine elevation.** "You've sent variants of the same '5 min before meeting' Slack ping 14 times this month. Want me to make it automatic on Calendar event start - 5min?"

Each is the system **noticing** something + **proposing** action. The user approves, edits, rejects, or auto-approves (per trust gradient — see below).

---

## What Phase 6 must build

### A. Continuous-scan loop

A higher-order agent loop (the "Proactive Agent") that runs on a cadence — every 1h / 4h / nightly, configurable — and:

1. Queries Memory Tree + Phase-5 structured entities for "patterns of interest" (the user's stage, the user's data, the user's goals).
2. For each pattern detected, decides whether it's worth proposing action.
3. If yes, drafts a `Proposal` (a workflow-shaped object the user can review).
4. Queues the proposal in the user's inbox.

Implementation candidate: a new dedicated agent definition `proactive_agent` (sibling to `workflow_node`, `orchestrator`, etc.) with its own iteration budget + its own prompt that frames it as "the resident analyst — look for opportunities, draft proposals, do NOT execute."

### B. Pattern detector library

Small, focused, composable scanners for specific signals. Initial candidates:

- **Stalled entity** — entity hasn't moved stage in N days (parameterised per entity kind).
- **Silent reply** — outbound message sent, no inbound reply within expected window.
- **Repeated user action** — user has done the same thing N times in a recurring window → propose automation.
- **Anomaly** — metric this week vs trailing 4-week moving average outside ±X%.
- **Calendar gap** — newly-opened slot + warm leads who want time.
- **Inbound aging** — incoming message / mention older than user's typical reply latency.
- **Cross-source consistency check** — Stripe says customer cancelled; HubSpot still says active.

Each detector is its own small Rust module. The proactive agent runs them all on its scan tick + aggregates findings before deciding what to propose.

### C. Proposal queue + inbox UI

A new surface in the app — `/proposals` or surfaced as a notification rail — listing pending agent proposals:

- One row per proposal: title, why-proposed, draft workflow preview, approve / edit / reject / "auto-approve this pattern in the future" buttons.
- The proposal is RENDERED as a `<WorkflowProposalPreview>` (F-14 component, reused) — same shape as drafter-emitted proposals today.
- Approve → fire the workflow (or save + enable, depending on the proposal's recommended trust level).
- Reject → store the rejection as a signal for the proactive agent to learn from (don't propose this again, or propose less often).

### D. Trust gradient — per-pattern, not per-workflow

This is the load-bearing safety design. The user must control how autonomous the agent is, granularly.

**Levels (per pattern):**

- **L0 — Supervised (default for all patterns).** Every proposal of this pattern requires user approval. User sees proposal, clicks approve / reject / edit.
- **L1 — Auto-fire with notification.** After N consecutive approvals of the SAME pattern (default N=5), system offers to upgrade that pattern to L1. Future proposals fire automatically + show up in a "Recently auto-fired" feed the user can audit.
- **L2 — Auto-fire silent.** Only available for patterns the user explicitly upgrades from L1. The audit feed still exists, but no notification per fire.

**Per-pattern, NOT per-workflow.** "Auto-approve my daily morning digest" is fine; "auto-approve sending cold emails to new leads" might be perma-L0. The granularity lives at the pattern detector, not the workflow.

**Auto-revert.** If a user manually rejects 2 consecutive proposals from an L1 pattern, the pattern drops back to L0. The system learns from the rejection.

### E. Measurement loop — close the feedback

Phase 6 needs Phase 5's outcome observability AND extends it: every approved proposal eventually has an outcome (Slack DM sent → did the lead reply? Calendar event created → did the meeting happen? Workflow proposed → did it convert?). Outcome data feeds back into:

- **Pattern detector weights.** Patterns whose proposals convert get scanned more aggressively. Patterns whose proposals get rejected get scanned less aggressively.
- **Trust-gradient thresholds.** A pattern that converts 90% of the time should reach L1 faster than one that converts 30% of the time.
- **Per-pattern ROI surfaces.** "Stalled-lead pattern has saved you 4hr/week, generated $48k in revenue this quarter."

---

## What Phase 6 explicitly is NOT

- **Not a generic autonomous-agent platform.** Phase 6 is scoped to: business-relevant patterns in THIS user's data, proposing actions within the user's existing Composio / Channel / Browser scopes.
- **Not unsupervised autopilot.** Default trust gradient is L0. L1/L2 are user-granted, per-pattern, revocable. The system is never running "open-loop" without the user being able to see and stop.
- **Not pattern discovery via ML / clustering.** Phase 6.1 ships the hand-coded pattern detector library. Discovering new patterns from data is a Phase 7+ research direction.
- **Not a public marketplace for patterns.** All patterns live in OpenHuman's codebase. Phase 7+ might open a community contribution model; not Phase 6.
- **Not separate from the existing workflow runtime.** Proposals materialize as regular `WorkflowProposal` objects → existing executor runs them. Phase 6 is the proactive LAYER, not a new execution engine.

---

## Dependencies

| Depends on | Why |
|---|---|
| Phase 1.5 F-17 (memory loop) | Phase 6's pattern detection reads `WorkflowRunMemory` to spot repeated actions worth automating + uses `actual` (ground-truth) for outcome learning. |
| Phase 2 (multi-node + retries + webhook/event triggers) | Phase 6's drafted proposals are multi-node workflows, often. |
| Phase 3 (browser agent) | Many high-value proposals need browser-agent capability (LinkedIn outreach drafts, niche CRM updates). |
| Phase 5 (structured entities + outcome observability) | Pattern detectors query structured tables (`leads`, `deals`, `proposals`). Trust-gradient learning needs outcome attribution. |
| F-17 entity_tags accumulated for 3–6 months | Without real tag data, we don't know which patterns matter. |

---

## Anti-scope-creep / what could go wrong

- **Building too many pattern detectors before any are validated.** Start with 3–5 detectors, ship them, measure conversion. Only add new ones after the first batch proves useful.
- **Premature trust-gradient automation.** L1 / L2 should require explicit user action. Never auto-promote without consent. Better to under-trust than over-trust.
- **Pattern detectors as LLM calls vs deterministic code.** Each pattern detector should be deterministic Rust (fast + cheap + auditable). The LLM call comes in step 3 ("draft the proposal") — not in step 1 ("scan for the pattern"). LLM-driven pattern detection is enormously expensive at scan-tick cadence; don't do it.
- **Hijacking the user's inbox with noise.** Cap the proposal queue at N pending items (default 10). When full, the proactive agent stops queueing until the user clears space. Otherwise the inbox becomes spam.

---

## Reading order for the future implementer

1. This placeholder.
2. F-17's closure DEVLOG entry (when written) — confirm `entity_tags` data is rich enough.
3. Phase 5's closure (when written) — confirm structured entity layer is queryable.
4. 2–4 months of using Phase 5 + F-17 in production. Keep a journal of "moments where I wished the system had noticed and proposed."
5. The journal IS the initial pattern detector list. Draft Phase 6 properly at that point.

Don't skip step 4. Theoretical patterns won't survive contact with real workflow.

---

## Why captured now, before being built

The architectural shape of Phases 2-5 will subtly bias toward or against Phase 6 viability depending on choices made along the way. If we keep the proactive-agent target in mind, the Phase 2/3/5 decisions will naturally stay compatible (workflows can be dispatched by any source not just user-clicked; entity tags accumulate; outcome observability gets first-class treatment). If we forget, those decisions will accumulate small assumptions that make Phase 6 painful or impossible.

This placeholder is the architectural North Star, not a build spec.
