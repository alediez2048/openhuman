// @ts-nocheck
/**
 * Phase 1 acceptance E2E — starter-catalog flow on a fresh workspace
 * (NFR-2.6.4).
 *
 * Validates the F-5 + F-6 catalog surface end-to-end:
 *
 *   1. /workflows route renders with the 4 bundled starter templates
 *      (RU-1..RU-4) listed in the "Starter workflows" section.
 *   2. Clicking [Add] on RU-1 persists a workflow row (oracle:
 *      workflows_list RPC returns exactly one Seed-origin row whose
 *      template_id is RU-1).
 *   3. The catalog re-renders without RU-1 (dedup against the
 *      Seed{template_id} row that just landed).
 *   4. Deleting that row (via the direct workflows_delete RPC, since
 *      the overflow-menu Delete path is a Phase 2 UI piece per the
 *      F-12 deferred propose-tool wiring) re-introduces RU-1 in the
 *      catalog on the next refresh.
 *
 * Follows the connections-hub.spec.ts template: one Appium session,
 * one resetApp() to walk a clean onboarding, then real UI
 * interactions backed by direct-RPC oracles.
 *
 * NOT exercised (deferred to Phase 1.5 / Phase 2):
 *   - Hero E2E (NFR-2.6.3): chat → propose → preview → Save & Enable
 *     — requires the chat-runtime protocol extension and the
 *     drafter agent invocations (F-11 / F-12 placeholders). The
 *     components ship in F-14; the integration is its own ticket.
 *   - Delete via the overflow-menu / WorkflowDeletePreview path —
 *     same chat-protocol dependency; the workflows_delete RPC is
 *     fully wired today.
 */
import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc } from '../helpers/core-rpc';
import { waitForText } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-workflows-seeded';
const RU_1_TEMPLATE_ID = 'ru-1-founder-morning-digest';

function stepLog(msg: string, ctx?: unknown): void {
  const stamp = new Date().toISOString();
  if (ctx === undefined) {
    console.log(`[WorkflowsSeededE2E][${stamp}] ${msg}`);
  } else {
    console.log(`[WorkflowsSeededE2E][${stamp}] ${msg}`, JSON.stringify(ctx));
  }
}

/** Wait for a `data-testid` element to be present in the renderer DOM. */
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

/** Wait for a `data-testid` element to be absent from the renderer DOM. */
async function waitForTestIdMissing(testId: string, timeoutMs = 10_000): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const absent = await browser.execute(
      (id: string) => !document.querySelector(`[data-testid="${id}"]`),
      testId
    );
    if (absent) return true;
    await browser.pause(300);
  }
  return false;
}

async function clickByTestId(testId: string): Promise<void> {
  await browser.execute((id: string) => {
    const el = document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
    if (!el) throw new Error(`testid "${id}" not found for click`);
    el.click();
  }, testId);
}

async function countStarterCards(): Promise<number> {
  return (await browser.execute(() => {
    return document.querySelectorAll('[data-testid^="starter-workflow-card-"]').length;
  })) as number;
}

async function countYourWorkflowsCards(): Promise<number> {
  return (await browser.execute(() => {
    return document.querySelectorAll('[data-testid^="workflow-card-"]').length;
  })) as number;
}

describe('Workflows — starter catalog flow (NFR-2.6.4)', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('renders the four bundled RU-1..RU-4 starter templates', async () => {
    stepLog('navigating to /workflows');
    await navigateViaHash('/workflows');
    await waitForText('Workflows', 15_000);

    // The four starter templates ship bundled — every fresh workspace
    // sees all four until the user adds one (per F-5 / F-6).
    expect(await waitForTestId(`starter-workflow-card-${RU_1_TEMPLATE_ID}`, 10_000)).toBe(true);
    expect(await countStarterCards()).toBe(4);
  });

  it('Add on RU-1 persists a workflow + dedupes from the catalog', async () => {
    stepLog('clicking Add on RU-1');
    await clickByTestId(`starter-workflow-add-${RU_1_TEMPLATE_ID}`);

    // Oracle: workflows_list RPC must reflect the new row tagged with
    // the matching Seed{template_id} origin.
    const deadline = Date.now() + 15_000;
    let workflows: Array<{ id: string; name: string; origin: { type: string; template_id?: string } }> = [];
    while (Date.now() < deadline) {
      const out = await callOpenhumanRpc('workflows_list', {});
      workflows = (out?.workflows ?? out) as typeof workflows;
      if (workflows.length >= 1) break;
      await browser.pause(400);
    }
    expect(workflows.length).toBe(1);
    const seeded = workflows[0]!;
    expect(seeded.origin.type).toBe('seed');
    expect(seeded.origin.template_id).toBe(RU_1_TEMPLATE_ID);

    // Catalog must re-render without RU-1.
    expect(await waitForTestIdMissing(`starter-workflow-card-${RU_1_TEMPLATE_ID}`, 10_000)).toBe(true);
    expect(await countStarterCards()).toBe(3);

    // Your-workflows section now shows the seeded row.
    expect(await waitForTestId(`workflow-card-${seeded.id}`, 5_000)).toBe(true);
    expect(await countYourWorkflowsCards()).toBe(1);
  });

  it('Delete restores RU-1 in the catalog on next refresh', async () => {
    // Oracle-driven delete via the wired RPC. The propose-then-click
    // flow (workflow_propose_delete → <WorkflowDeletePreview> →
    // workflows_delete) requires the chat-runtime protocol
    // extension that's deferred to Phase 1.5; the workflows_delete
    // RPC itself is fully tested in the Rust suite.
    const listed = (await callOpenhumanRpc('workflows_list', {}))?.workflows ?? [];
    expect(listed.length).toBe(1);
    const wf = listed[0]!;
    stepLog(`deleting ${wf.id} via direct RPC`);
    await callOpenhumanRpc('workflows_delete', { id: wf.id });

    // Yourworkflows section empties; catalog regrows back to 4.
    expect(await waitForTestIdMissing(`workflow-card-${wf.id}`, 10_000)).toBe(true);
    expect(await waitForTestId(`starter-workflow-card-${RU_1_TEMPLATE_ID}`, 10_000)).toBe(true);
    expect(await countStarterCards()).toBe(4);

    // Confirmation via the RPC: zero workflows persisted.
    const afterDelete = (await callOpenhumanRpc('workflows_list', {}))?.workflows ?? [];
    expect(afterDelete.length).toBe(0);
  });
});
