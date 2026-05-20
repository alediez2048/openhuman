# `<WorkflowProposalPreview>` — Design Spec

> Synthesized from four parallel sub-agent designs (Minimalist · Inspectable · Conversational · Diff-card). Locked.
>
> **Final location:** `app/src/components/workflows/preview/WorkflowProposalPreview.tsx`. Sibling components at `WorkflowEditPreview.tsx`, `WorkflowDeletePreview.tsx`, `WorkflowStatePreview.tsx`.
>
> **Status:** Phase 1 design, ready for ticket reference.

---

## Composition principle

**Minimalist by default, inspectable on disclosure, conversational on save.** Specifically:

1. **Base render = minimalist** (~140px tall). Name · one-line description · trigger summary · connection chips · three action buttons. That's it.
2. **"Show details" disclosure** expands an inspectable panel below the card with Rationale · Full agent prompt · Required connections detail · Settings. Sectioned, collapsed-within-the-panel by default. **No "Raw JSON" section** — that's developer-tool noise.
3. **Saved-state = conversational morph.** After a successful Save, the card animates into a one-line "✓ Saved as X" stub *and* a new agent message bubble renders below ("Done. You'll find it in /workflows. Want me to add Y too?"). The save reads as the agent continuing, not a form completing.

The **edit case** uses a separate `<WorkflowEditPreview>` with diff-row rendering (proven shape from the diff-card design exploration). The **delete case** uses `<WorkflowDeletePreview>` — a simple coral-bordered confirmation card. The **state-toggle / run-now** cases use `<WorkflowStatePreview>` — a tighter version of the proposal card with a single primary button.

---

## Default render — pending state

```
┌───────────────────────────────────────────────────────────────┐
│  ⚡  Founder morning digest                       ● high      │
│  Weekday 8am triage across Gmail, Linear, Slack → Telegram.  │
│                                                               │
│  ⏰  Every weekday at 8am  ·  🔌  gmail  linear  slack  tg    │
│                                                               │
│  ⌄ Show details                                               │
│  [ Discard ]              [ Save (paused) ]   [ Save & Enable ▸ ] │
└───────────────────────────────────────────────────────────────┘
```

- Card: `bg-white rounded-2xl shadow-soft border border-ocean-100 p-4 max-w-[560px]`.
- Lightning glyph (`⚡` rendered as `<BoltIcon>`): ocean primary.
- Title: `font-semibold text-stone-900 text-base`.
- Description: `text-sm text-stone-500 mt-0.5`.
- Confidence dot: `● high` (sage) / `● medium` (amber) / `● low` (coral).
- Trigger line: humanized via `cronstrue` (e.g., `Every weekday at 8am`). For `manual`, shows `Run on demand`.
- Connection chips: small pill-shaped, toolkit/provider name only. Missing ones get an amber `⚠` prefix and amber border-left accent.
- `Show details` chevron: `text-xs text-ocean-600 hover:underline`.
- Primary action: `Save & Enable` (ocean fill). Secondary: `Save (paused)` (ocean ghost). Tertiary: `Discard` (muted text-only).

When `missing_connections.length > 0`, an amber banner replaces the action row's first slot:

```
│  ⚠ Connect linkedin, twitter to enable           [ Manage connections ▸ ] │
│  [ Discard ]              [ Save (paused) ]   ⟨ Save & Enable ⟩ disabled │
```

`Save (paused)` remains enabled — the workflow saves with `health: NeedsConnections { missing }` and lives paused until the connections are set up.

When `confidence === 'low'`, the rationale auto-expands inside the details panel; otherwise it's collapsed.

When `setup_instructions != null`, an amber callout banner sits above the action row.

---

## Expanded — Show details clicked

```
┌───────────────────────────────────────────────────────────────┐
│  ... base card render above ...                               │
│  ⌃ Hide details                                               │
│                                                               │
│  ▸ Rationale                                          (4)    │
│  ▸ Agent prompt                                      preview │
│  ▸ Required connections                             (4/4)    │
│  ▸ Settings                                                  │
│                                                               │
│  [ Discard ]              [ Save (paused) ]   [ Save & Enable ▸ ] │
└───────────────────────────────────────────────────────────────┘
```

Each `▸` is a `<DetailsSection>` collapsible. Open one at a time (radio behavior). The first-line of the prompt is shown next to "Agent prompt" so users can scan without expanding.

Expanded sections:

- **Rationale** — bullet list of `rationale: string[]`.
- **Agent prompt** — full prompt text in a `<pre>` block, `font-mono text-xs`, `max-h-64 overflow-auto`. Copy button. *No "expand to modal"* — keeps the chat-thread focus.
- **Required connections** — table: provider · account · status (`Connected` / `Not connected` with a `Connect →` link).
- **Settings** — key-value pairs: `iteration_cap`, `model_tier`, `timeout_secs`, `on_error`. Read-only. Edits happen post-save via chat.

---

## Saved state — conversational morph

After Save succeeds, the card transitions in 300ms (`ease-out`, respect `prefers-reduced-motion`):

```
┌───────────────────────────────────────────────────────────────┐
│  ✓ Saved as "Founder morning digest"                          │
│   Paused · View ▸  ·  Enable now                              │
└───────────────────────────────────────────────────────────────┘
```

Sage left border (`border-l-4 border-sage-500`). ~56px tall. The original action row unmounts.

**Immediately below**, a new agent-message bubble renders (post-save synthetic user message → agent's next turn):

```
   OpenHuman · just now
   Done. You'll find it in /workflows under "Your workflows."
   Want me to add a Friday afternoon reflection workflow too?
```

This makes the save feel like the agent kept talking. The next user turn continues the conversation naturally.

For `Save & Enable`, the stub instead reads:

```
│  ✓ Saved & enabled — "Founder morning digest"                │
│   Next run: tomorrow 8:00 AM UTC · View ▸                     │
```

---

## State machine

```
              ┌──────────────────────┐
              │       pending        │
              │  (default; visible   │
              │   buttons live)      │
              └─────┬──────┬─────────┘
                    │      │ Discard click
                    │      ▼
                    │ ┌─────────────┐
                    │ │  discarded  │  (muted stub: "Discarded — Undo (15s)")
                    │ └─────────────┘
                    │
        Save / Save&Enable click
                    │
                    ▼
              ┌─────────────┐
              │   saving    │  (buttons → spinner pill; card freezes)
              └──────┬──────┘
              ┌──────┴──────┐
              ▼             ▼
       ┌─────────────┐   ┌──────────┐
       │   saved     │   │  error   │  (coral border; Retry / Discard)
       │ (sage stub  │   └────┬─────┘
       │  + agent    │        │ Retry
       │  bubble)    │        ▼
       └─────────────┘     pending
```

`saved`, `discarded` are terminal in the thread (chat history is immutable). Re-opening the thread shows the terminal state.

---

## Component API

```tsx
interface WorkflowProposalPreviewProps {
  proposal: WorkflowProposal;
  threadId: string;
  state?: 'pending' | 'saving' | 'saved' | 'error' | 'discarded';
  errorMessage?: string;
  onSavePaused: (proposal: WorkflowProposal) => Promise<void>;
  onSaveAndEnable: (proposal: WorkflowProposal) => Promise<void>;
  onDiscard: () => void;
  onManageConnections?: (missing: ConnectionRef[]) => void;
}

// Child components — internal, none exported:
<ProposalHeader name confidence />
<TriggerLine trigger />
<ConnectionChips required missing onManage />
<SetupInstructionsCallout text />
<MissingConnectionsBanner missing onManage />
<DetailsDisclosure expanded onToggle />
<DetailsPanel proposal>
  <Section label="Rationale" badge={count}><RationaleBullets/></Section>
  <Section label="Agent prompt" preview={firstLine}><PromptViewer text/></Section>
  <Section label="Required connections" badge="{n}/{m}"><ConnectionsTable/></Section>
  <Section label="Settings"><SettingsTable/></Section>
</DetailsPanel>
<ActionRow state primary secondary tertiary />
<SavedStub mode="paused" | "enabled" workflowId name />
```

Hooks:
- `useWorkflowProposalActions(proposal, threadId)` — wraps `workflows_create` + `workflows_enable` from `services/api/workflows.ts`. Posts the synthetic *"Saved as X."* user message on success.
- `useCronHumanizer(expr, tz)` — pure helper.
- `useConnectionMeta(refs)` — fetches display labels + icons from a static registry (no network).

---

## What's hidden (the contract)

Hidden by default (revealed only via `Show details`):
- The full `agent_prompt.prompt` text (often 100+ words).
- `rationale[]` (unless `confidence === 'low'`).
- `iteration_cap`, `model_tier`, `timeout_secs`, `on_error`.
- Individual `account_id` / `connection_id` / `integration` strings on `ConnectionRef`s.

Hidden always (never on this surface):
- Raw JSON of the proposal.
- `proposal_id` / internal correlation tokens.
- Edge graph (Phase 1 has none).
- Node graph topology beyond "1 step" (Phase 1 always single-node).

---

## Companion components (separate spec, summarized)

### `<WorkflowEditPreview>` — diff-card pattern

Edit cases use diff rendering (justified — they ARE diffs). Same header shape, but rows have `+`/`-`/` ` gutters showing current → proposed:

```
┌───────────────────────────────────────────────────────────────┐
│  ✏  Founder morning digest — proposed edit                    │
├───────────────────────────────────────────────────────────────┤
│   trigger.cron                                                │
│ - 0 8 * * 1-5                                                 │
│ + 0 9 * * 1-5     # change schedule to 9am                    │
├───────────────────────────────────────────────────────────────┤
│   agent_prompt.prompt                                         │
│   …                                                           │
│ + Also include unread Slack DMs in #design.                   │
├───────────────────────────────────────────────────────────────┤
│  [ Cancel ]                              [ Apply changes ▸ ]  │
└───────────────────────────────────────────────────────────────┘
```

Avoids the diff-card design's developer-tool tells (`#abc123` hash, `trigger.cron` file-path labels). Uses human labels (`Trigger schedule`, `Agent prompt`).

### `<WorkflowDeletePreview>` — coral confirmation

```
┌───────────────────────────────────────────────────────────────┐
│  🗑  Delete "Founder morning digest"                          │
│  This workflow has 14 past runs. Run history is kept for 30   │
│  days after deletion, then permanently purged.                │
│                                                               │
│  [ Cancel ]                                       [ Delete ]  │
└───────────────────────────────────────────────────────────────┘
```

Coral border-left. Destructive button styled coral fill.

### `<WorkflowStatePreview>` — for toggle / run-now

```
┌───────────────────────────────────────────────────────────────┐
│  ▶  Run "Founder morning digest" now?                         │
│  Estimated time: ~20s. The next scheduled run is still on.    │
│                                                               │
│  [ Cancel ]                                  [ Run now ▸ ]    │
└───────────────────────────────────────────────────────────────┘
```

Smaller card (~80px). Single primary button.

---

## Accessibility

- All buttons reachable via keyboard tab order. Discard / Save (paused) / Save & Enable in that order so the primary lands last (matches macOS dialog convention).
- Confidence dot: `aria-label="confidence: high"`.
- Connection chips: each chip is a `<button>` if `onManage` is provided, focusable.
- Details disclosure: standard `<details><summary>` semantics so screen readers announce expanded state.
- Saving state: `aria-busy=true` on the card root; `aria-live="polite"` region announces "Saving workflow…" / "Saved." / "Couldn't save — <reason>."
- Respect `prefers-reduced-motion` for the saved-state morph.

---

## i18n

Translatable strings:
- `Show details` / `Hide details`
- `Discard` / `Save (paused)` / `Save & Enable`
- `Rationale` / `Agent prompt` / `Required connections` / `Settings`
- `Connected` / `Not connected` / `Connect →`
- `Saved as` / `Paused` / `Enable now`
- `Couldn't save — {reason}` / `Retry`
- `Discarded — Undo` / state-toggle / delete confirmations
- Confidence labels: `high` / `medium` / `low` (rendered as keyed enums for translation).

Cron humanization comes from `cronstrue` which supports the locale chunks OpenHuman already ships.

---

## Implementation order (informs ticket sequence)

1. **Static render** — `WorkflowProposalPreview` rendering the pending state from a fixture payload. No actions wired. Vitest snapshot test.
2. **Details panel** — disclosure + four sections + prompt viewer. Vitest test per section.
3. **Connection chips + missing banner** — wire to `useConnectionMeta`. Vitest test for missing-connections variant.
4. **Action wiring** — `useWorkflowProposalActions` + state machine. Mock the RPC clients; assert the synthetic user message is posted on success.
5. **Saved-state morph** — animation + agent-bubble follow-up. Visual regression via storybook screenshot.
6. **Error + Discard states** — coral border, Retry, Undo. Vitest tests.
7. **Companion components** — `WorkflowEditPreview`, `WorkflowDeletePreview`, `WorkflowStatePreview`. Share `<ProposalHeader>` and `<ActionRow>`.
8. **Chat-runtime registration** — register all four components with the rich-message renderer.
9. **E2E hero flow** — covers description → propose → preview render → Save & Enable click → workflows list → run completes.
