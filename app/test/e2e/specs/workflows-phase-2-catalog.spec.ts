// @ts-nocheck
/**
 * Phase 2 catalog acceptance E2E — F2-16b.
 *
 * Companion to `workflows-seeded.spec.ts` (Phase 1 catalog of 4
 * templates). Phase 2 ships RU-5..RU-9 alongside the original four,
 * so a fresh workspace MUST render 9 starter cards. Validates the
 * F2-12 + F2-16 surface end-to-end:
 *
 *   1. /workflows renders 9 starter cards (RU-1..RU-9).
 *   2. Clicking [Add] on RU-7 (GitHub webhook summary) persists a
 *      workflow row whose origin is Seed { template_id:
 *      "ru-7-github-issue-summary" } and dedupes the matching
 *      catalog card.
 *   3. Direct-RPC `workflows_delete` (Phase 2 soft-delete per F2-14)
 *      releases the template back into the catalog on next refresh.
 *
 * NOT exercised here (deferred to F2-17 — the comprehensive Appium
 * live-testing pass):
 *   - Hero chat-driven flow ("describe an automation → preview →
 *     [Save & Enable]"). Requires the same mock-LLM + drafter
 *     scaffolding F2-17 will land; F2-16b stays focused on the
 *     catalog surface so the two specs aren't competing for the
 *     same harness.
 *   - Webhook POST → run dispatch end-to-end (F2-17 scenario 2).
 *   - Multi-node chain execution (F2-17 scenario 1).
 */
import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc } from '../helpers/core-rpc';
import { waitForText } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-workflows-phase-2-catalog';
const RU_7_TEMPLATE_ID = 'ru-7-github-issue-summary';
const ALL_TEMPLATE_IDS = [
  'ru-1-founder-morning-digest',
  'ru-2-linkedin-engagement-queue',
  'ru-3-spotify-friday-five',
  'ru-4-jira-sprint-retro',
  'ru-5-stripe-payment-thank-you',
  'ru-6-slack-mention-triage',
  'ru-7-github-issue-summary',
  'ru-8-daily-sales-rollup',
  'ru-9-zapier-bridge',
];
const EXPECTED_STARTER_COUNT = ALL_TEMPLATE_IDS.length;

function stepLog(msg: string, ctx?: unknown): void {
  const stamp = new Date().toISOString();
  if (ctx === undefined) {
    console.log(`[WorkflowsPhase2CatalogE2E][${stamp}] ${msg}`);
  } else {
    console.log(`[WorkflowsPhase2CatalogE2E][${stamp}] ${msg}`, JSON.stringify(ctx));
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

describe('Workflows — Phase 2 starter catalog (F2-12 + F2-16)', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('renders all 9 bundled starter templates (4 Phase 1 + 5 Phase 2)', async () => {
    stepLog('navigating to /workflows');
    await navigateViaHash('/workflows');
    await waitForText('Workflows', 15_000);

    // Every fresh workspace MUST surface RU-1..RU-9 in the starter
    // section. Pin each card by id so a regression that drops a
    // single template fails with a clear "RU-X missing" message.
    for (const templateId of ALL_TEMPLATE_IDS) {
      const present = await waitForTestId(`starter-workflow-card-${templateId}`, 10_000);
      expect(present).toBe(true);
    }
    expect(await countStarterCards()).toBe(EXPECTED_STARTER_COUNT);
  });

  it('Add on RU-7 persists a Seed{ru-7} workflow + dedupes from the catalog', async () => {
    stepLog(`clicking Add on ${RU_7_TEMPLATE_ID}`);
    await clickByTestId(`starter-workflow-add-${RU_7_TEMPLATE_ID}`);

    // Oracle: workflows_list reflects exactly one Seed-origin row
    // matching RU-7.
    const deadline = Date.now() + 15_000;
    let workflows: Array<{
      id: string;
      name: string;
      origin: { type: string; template_id?: string };
    }> = [];
    while (Date.now() < deadline) {
      const out = await callOpenhumanRpc('workflows_list', {});
      workflows = (out?.workflows ?? out) as typeof workflows;
      if (workflows.length >= 1) break;
      await browser.pause(400);
    }
    expect(workflows.length).toBe(1);
    const seeded = workflows[0]!;
    expect(seeded.origin.type).toBe('seed');
    expect(seeded.origin.template_id).toBe(RU_7_TEMPLATE_ID);

    // Catalog re-renders without RU-7; remaining 8 still surface.
    expect(await waitForTestIdMissing(`starter-workflow-card-${RU_7_TEMPLATE_ID}`, 10_000)).toBe(
      true
    );
    expect(await countStarterCards()).toBe(EXPECTED_STARTER_COUNT - 1);

    // Your-workflows section now shows the seeded row.
    expect(await waitForTestId(`workflow-card-${seeded.id}`, 5_000)).toBe(true);
    expect(await countYourWorkflowsCards()).toBe(1);
  });

  it('Delete restores RU-7 in the catalog on next refresh (F2-14 soft-delete still releases templates)', async () => {
    // F2-14: workflows_delete is now a soft-delete. The catalog
    // dedup query (`list_seed_origins`) filters `deleted_at IS NULL`
    // so a soft-deleted seed releases its template back into the
    // catalog — the user can re-add it. Past the 30-day retention
    // window the retention sweep hard-deletes the row; here we just
    // confirm the immediate post-delete behaviour.
    const listed = (await callOpenhumanRpc('workflows_list', {}))?.workflows ?? [];
    expect(listed.length).toBe(1);
    const wf = listed[0]!;
    stepLog(`soft-deleting ${wf.id} via direct RPC`);
    await callOpenhumanRpc('workflows_delete', { id: wf.id });

    // Your-workflows section empties; catalog regrows to 9.
    expect(await waitForTestIdMissing(`workflow-card-${wf.id}`, 10_000)).toBe(true);
    expect(await waitForTestId(`starter-workflow-card-${RU_7_TEMPLATE_ID}`, 10_000)).toBe(true);
    expect(await countStarterCards()).toBe(EXPECTED_STARTER_COUNT);

    // The default `workflows_list` MUST exclude the soft-deleted row
    // (F2-14 semantics) — proves the catalog dedup goes through the
    // right query.
    const afterDelete = (await callOpenhumanRpc('workflows_list', {}))?.workflows ?? [];
    expect(afterDelete.length).toBe(0);

    // Trash-view variant: workflows_list { include_deleted: true }
    // still surfaces the row inside the 30-day retention window.
    const withDeleted =
      (await callOpenhumanRpc('workflows_list', { filter: { include_deleted: true } }))
        ?.workflows ?? [];
    expect(withDeleted.length).toBe(1);
    expect(withDeleted[0]!.id).toBe(wf.id);
  });
});
