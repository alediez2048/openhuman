// @ts-nocheck
/**
 * Phase 2 live-testing pass — F2-17.
 *
 * Comprehensive end-to-end coverage of the Phase 2 surface through
 * the macOS Appium (XCUITest :4723) harness against the real `.app`
 * bundle. Per the F2-17 primer (`Automations/Tickets/phase-2-execution/F2-17.md`),
 * the goal is the live-testing gate the user requires before treating
 * Phase 2 as "really shipped" (per the user-profile note: gates
 * everything through live testing).
 *
 * Strategy: one Appium session, one `resetApp()` walk, then a series
 * of independent `it()` blocks each driving one Phase 2 capability
 * end-to-end. Direct RPC seeds the workflows + asserts the runtime
 * state; the UI surface is exercised passively (workflow cards
 * render, capability list populates).
 *
 * Scenario coverage (F2-17 primer §Goal):
 *   1. Multi-node chain via direct RPC + run_now + poll for terminal
 *   2. [DEFERRED — needs tunnel harness] Webhook trigger end-to-end
 *   3. [DEFERRED — needs bus driver]      composio_event fan-out
 *   4. [DEFERRED — needs bus driver]      channel_message filter
 *   5. [DEFERRED — needs stub injection]  condition branching
 *   6. Soft-delete + restore round-trip (F2-14)
 *   7. active_hours validator rejection (F2-15 validator-side; the
 *      runtime gate is unit-tested via `scheduler::active_hours_skip`)
 *   8. Starter-catalog Phase 2 entries (F2-12)
 *   9. Capability surface (F2-16 about_app entries)
 *
 * The four DEFERRED scenarios are covered exhaustively by the Rust
 * unit + integration tests (345/345 workflows tests pass — see
 * Automations/Tickets/phase-2-execution/DEVLOG.md). What's missing is
 * the through-the-real-transport version; landing it cleanly needs
 * scaffolding (a tunnel-shaped HTTP receiver inside the test rig, a
 * `workflows_dev_simulate_trigger` debug RPC gated by env, etc.)
 * that's worth a dedicated F2-17b follow-up rather than inflating
 * this spec.
 */
import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc } from '../helpers/core-rpc';
import { waitForText } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-workflows-phase-2-live';

function stepLog(msg: string, ctx?: unknown): void {
  const stamp = new Date().toISOString();
  if (ctx === undefined) {
    console.log(`[WorkflowsPhase2LiveE2E][${stamp}] ${msg}`);
  } else {
    console.log(`[WorkflowsPhase2LiveE2E][${stamp}] ${msg}`, JSON.stringify(ctx));
  }
}

async function waitForTestId(testId: string, timeoutMs = 10_000): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const found = await browser.execute(
      (id: string) => Boolean(document.querySelector(`[data-testid="${id}"]`)),
      testId
    );
    if (found) return true;
    await browser.pause(300);
  }
  return false;
}

async function pollForTerminalRun(
  workflowId: string,
  timeoutMs = 30_000
): Promise<{ status: string; id: string; error?: string | null }> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const out = await callOpenhumanRpc('workflows_list_runs', {
      workflow_id: workflowId,
      pagination: { offset: 0, limit: 1 },
    });
    const runs = (out?.runs ?? out) as Array<{ id: string; status: string; error?: string | null }>;
    if (runs && runs[0] && !['pending', 'running'].includes(runs[0].status)) {
      return runs[0];
    }
    await browser.pause(400);
  }
  throw new Error(
    `run never reached terminal status for workflow=${workflowId} within ${timeoutMs}ms`
  );
}

/** Build a 3-node Phase 2 chain via direct RPC. Uses `tool_call`
 *  with the `passthrough` no-op tool so the chain is deterministic
 *  without needing a mock LLM. */
async function createPhase2Chain(workflowName: string): Promise<string> {
  const create = await callOpenhumanRpc('workflows_create', {
    request: {
      name: workflowName,
      description: 'F2-17 live-testing chain',
      trigger: { type: 'manual' },
      nodes: [
        {
          id: 'n1',
          kind: 'tool_call',
          config: {
            kind: 'tool_call',
            tool_name: 'passthrough',
            arguments_template: { stage: 'one', value: 42 },
          },
          position: null,
          retry_policy: null,
        },
        {
          id: 'n2',
          kind: 'tool_call',
          config: {
            kind: 'tool_call',
            tool_name: 'passthrough',
            arguments_template: { stage: 'two', upstream: '{{node.n1.output}}' },
          },
          position: null,
          retry_policy: null,
        },
        {
          id: 'n3',
          kind: 'tool_call',
          config: {
            kind: 'tool_call',
            tool_name: 'passthrough',
            arguments_template: { stage: 'three', upstream: '{{node.n2.output}}' },
          },
          position: null,
          retry_policy: null,
        },
      ],
      edges: [
        { from: 'n1', to: 'n2' },
        { from: 'n2', to: 'n3' },
      ],
      settings: null,
      origin: { type: 'user_chat' },
    },
  });
  const wf = (create?.workflow ?? create) as { id: string };
  return wf.id;
}

describe('Workflows — Phase 2 live-testing pass (F2-17)', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  // ── Scenario 1: multi-node chain ─────────────────────────────────────
  it('scenario 1: a 3-node chain executes end-to-end with step persistence + templating', async () => {
    const wfId = await createPhase2Chain('F2-17 chain');
    stepLog('chain created', { wfId });

    // Enable + run.
    await callOpenhumanRpc('workflows_enable', { id: wfId });
    const runStart = await callOpenhumanRpc('workflows_run_now', {
      id: wfId,
      initiator: { type: 'user' },
    });
    const runId = (runStart?.run_id ?? runStart) as string;
    expect(typeof runId).toBe('string');

    const run = await pollForTerminalRun(wfId);
    expect(run.status).toBe('succeeded');

    // Pull the full run + step rows.
    const full = await callOpenhumanRpc('workflows_get_run', { run_id: run.id });
    const steps = (full?.steps ?? []) as Array<{
      node_id: string;
      status: string;
      output_json?: string | null;
    }>;
    expect(steps.length).toBe(3);
    expect(steps.map(s => s.node_id)).toEqual(['n1', 'n2', 'n3']);
    for (const s of steps) {
      expect(s.status).toBe('succeeded');
    }
    // Templating proof: n3's output_json must mention "two" (upstream
    // chain travelled n1 → n2 → n3 through `{{node.<id>.output}}`).
    expect(steps[2]!.output_json).toContain('two');
  });

  // ── Scenario 6: soft-delete + restore (F2-14) ────────────────────────
  it('scenario 6: workflows_delete soft-deletes; workflows_restore brings the row + history back', async () => {
    const wfId = await createPhase2Chain('F2-17 soft-delete victim');
    stepLog('workflow seeded for soft-delete', { wfId });

    // Soft-delete.
    await callOpenhumanRpc('workflows_delete', { id: wfId });

    // Default list excludes the row.
    const defaultList = (await callOpenhumanRpc('workflows_list', {}))?.workflows ?? [];
    expect(defaultList.find((w: { id: string }) => w.id === wfId)).toBeUndefined();

    // include_deleted surfaces it (Trash view).
    const trashList =
      (await callOpenhumanRpc('workflows_list', { filter: { include_deleted: true } }))
        ?.workflows ?? [];
    expect(trashList.find((w: { id: string }) => w.id === wfId)).toBeDefined();

    // Restore brings it back.
    const restoredOutcome = await callOpenhumanRpc('workflows_restore', { id: wfId });
    const restored = (restoredOutcome?.workflow ?? restoredOutcome) as { id: string } | null;
    expect(restored).not.toBeNull();
    expect(restored!.id).toBe(wfId);

    const afterRestore = (await callOpenhumanRpc('workflows_list', {}))?.workflows ?? [];
    expect(afterRestore.find((w: { id: string }) => w.id === wfId)).toBeDefined();
  });

  // ── Scenario 7: active_hours validator rejection (F2-15) ─────────────
  it('scenario 7: workflows_create rejects malformed / out-of-order active_hours', async () => {
    // Start < end inverted — validator MUST reject.
    let rejected = false;
    try {
      await callOpenhumanRpc('workflows_create', {
        request: {
          name: 'F2-17 bad active_hours',
          description: 'should reject',
          trigger: {
            type: 'cron',
            expr: '*/15 * * * *',
            tz: null,
            active_hours: { start: '17:00', end: '09:00' },
          },
          nodes: [
            {
              id: 'n1',
              kind: 'tool_call',
              config: { kind: 'tool_call', tool_name: 'passthrough', arguments_template: {} },
              position: null,
              retry_policy: null,
            },
          ],
          edges: [],
          settings: null,
          origin: { type: 'user_chat' },
        },
      });
    } catch (err) {
      rejected = true;
      stepLog('validator rejected as expected', { err: String(err) });
    }
    expect(rejected).toBe(true);

    // Valid window must pass.
    const ok = await callOpenhumanRpc('workflows_create', {
      request: {
        name: 'F2-17 valid active_hours',
        description: 'should accept',
        trigger: {
          type: 'cron',
          expr: '*/15 * * * *',
          tz: null,
          active_hours: { start: '09:00', end: '17:00' },
        },
        nodes: [
          {
            id: 'n1',
            kind: 'tool_call',
            config: { kind: 'tool_call', tool_name: 'passthrough', arguments_template: {} },
            position: null,
            retry_policy: null,
          },
        ],
        edges: [],
        settings: null,
        origin: { type: 'user_chat' },
      },
    });
    expect((ok?.workflow ?? ok)?.id).toBeTruthy();
  });

  // ── Scenario 8: starter catalog Phase 2 entries (F2-12) ──────────────
  it('scenario 8: list_starter_templates(phase=2) returns all 9 RU-1..RU-9 templates', async () => {
    const out = await callOpenhumanRpc('workflows_list_starter_templates', { phase: 2 });
    const templates = (out?.templates ?? out) as Array<{ template_id: string }>;
    const ids = new Set(templates.map(t => t.template_id));
    for (const id of [
      'ru-1-founder-morning-digest',
      'ru-2-linkedin-engagement-queue',
      'ru-3-spotify-friday-five',
      'ru-4-jira-sprint-retro',
      'ru-5-stripe-payment-thank-you',
      'ru-6-slack-mention-triage',
      'ru-7-github-issue-summary',
      'ru-8-daily-sales-rollup',
      'ru-9-zapier-bridge',
    ]) {
      expect(ids.has(id)).toBe(true);
    }
  });

  // ── Scenario 9: Phase 2 capability surface (F2-16) ───────────────────
  it('scenario 9: about_app_list_capabilities returns the 7 Phase 2 entries', async () => {
    const out = await callOpenhumanRpc('about_app_list_capabilities', {});
    const caps = (out?.capabilities ?? out) as Array<{ id: string }>;
    const ids = new Set(caps.map(c => c.id));
    for (const id of [
      'automation.multi_node_chain',
      'automation.webhook_trigger',
      'automation.composio_event_trigger',
      'automation.channel_message_trigger',
      'automation.node_retry_policy',
      'automation.workflow_soft_delete',
      'automation.active_hours_gate',
    ]) {
      expect(ids.has(id)).toBe(true);
    }
  });

  // ── UI surface sanity — /workflows still renders the Phase 2 catalog ──
  it('UI: navigating to /workflows renders the Phase 2 starter section', async () => {
    await navigateViaHash('/workflows');
    await waitForText('Workflows', 15_000);
    // Pick one Phase 2 card as a render canary; absence here means the
    // catalog rendering broke under Phase 2 even though the RPC works.
    expect(await waitForTestId('starter-workflow-card-ru-7-github-issue-summary', 10_000)).toBe(
      true
    );
  });

  // ── Deferred scenarios — explicit skips with the reason ──────────────
  it.skip('scenario 2 (DEFERRED → F2-17b): webhook POST → run dispatches with payload templating', () => {
    // Needs a tunnel-shaped HTTP receiver inside the test rig that
    // can POST to the registered tunnel URL with a valid HMAC. The
    // executor's `dispatch_run_with_payload` + cap + reconcile are
    // all unit-tested in the Rust suite (F2-9, F2-9b); the gap is
    // through-the-transport verification.
  });
  it.skip('scenario 3 (DEFERRED → F2-17b): composio_event fan-out dispatches all matching workflows', () => {
    // Needs a `workflows_dev_simulate_trigger` debug RPC gated by
    // OPENHUMAN_DEV_E2E=1. The fan-out + filter logic is covered by
    // F2-10's executor + bus tests.
  });
  it.skip('scenario 4 (DEFERRED → F2-17b): channel_message filter dispatches only matching messages', () => {
    // Same debug-RPC dependency as scenario 3 — F2-11 logic is
    // covered by the Rust unit suite.
  });
  it.skip('scenario 5 (DEFERRED → F2-17b): condition node routes between then/else branches', () => {
    // Needs a `workflows_dev_inject_agent_stub` debug RPC so the
    // condition can be driven against deterministic agent outputs.
    // F2-6 reachability + branch walk is covered by Rust unit tests.
  });
});
