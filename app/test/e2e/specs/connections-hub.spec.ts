// @ts-nocheck
/**
 * Phase 0 acceptance E2E — Connections Hub flow on a fresh workspace.
 *
 * Validates the full Phase 0 surface (P0-1..P0-6) end-to-end:
 *
 *   1. `/connections` route renders the unified hub with all 6 sections.
 *   2. The legacy `/skills` route redirects to `/connections`.
 *   3. The legacy `/channels` route deep-links to `/connections#channels`.
 *   4. Creating a Generic HTTP connection through the real modal UI
 *      persists a row (oracle: connections_list RPC reflects the new
 *      entry) and renders a card.
 *   5. The card's Delete affordance removes the row (oracle: subsequent
 *      connections_list omits it).
 *   6. The search input narrows the visible cards.
 *
 * Follows the cron-jobs-flow.spec.ts template: one Appium session, one
 * resetApp() to walk a clean onboarding, then real UI interactions.
 *
 * NOTE: Phase 0 deferred follow-ups are NOT exercised here:
 *   - P0-6a/b (built-in toggle + MCP restart) — no UI affordance yet.
 *   - P0-5a/b/c (per-mechanism deep CRUD reuse from legacy Skills page).
 */
import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc } from '../helpers/core-rpc';
import { textExists, waitForText } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-connections-hub';
const CONNECTION_NAME = 'e2e-test-conn';
const BASE_URL = 'http://127.0.0.1:0/echo'; // P0-3 test probe is stubbed; URL is not contacted.

function stepLog(msg: string, ctx?: unknown): void {
  const stamp = new Date().toISOString();
  if (ctx === undefined) {
    console.log(`[ConnectionsHubE2E][${stamp}] ${msg}`);
  } else {
    console.log(`[ConnectionsHubE2E][${stamp}] ${msg}`, JSON.stringify(ctx));
  }
}

/** Returns the URL hash fragment after `#`. */
async function currentHash(): Promise<string> {
  const url = await browser.getUrl();
  const idx = url.indexOf('#');
  return idx === -1 ? '' : url.slice(idx + 1);
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

/** Fill a text input identified by `data-testid` and dispatch a real input event. */
async function setInputByTestId(testId: string, value: string): Promise<void> {
  await browser.execute(
    (id: string, v: string) => {
      const el = document.querySelector<HTMLInputElement>(`[data-testid="${id}"]`);
      if (!el) throw new Error(`input testid "${id}" not found`);
      const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value');
      setter?.set?.call(el, v);
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    },
    testId,
    value
  );
}

/** Click an element identified by `data-testid` via real DOM dispatch. */
async function clickByTestId(testId: string): Promise<void> {
  const ok = await browser.execute((id: string) => {
    const el = document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
    if (!el) return false;
    el.click();
    return true;
  }, testId);
  if (!ok) throw new Error(`clickByTestId: testid "${testId}" not found`);
}

describe('Connections Hub — Phase 0 acceptance', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('renders all 6 sections on a fresh workspace', async () => {
    stepLog('navigating to /connections');
    await navigateViaHash('/connections');
    await waitForText('Connections', 15_000);

    for (const section of [
      'connections-section-composio',
      'connections-section-channels',
      'connections-section-webview',
      'connections-section-builtin',
      'connections-section-mcp',
      'connections-section-generic-http',
    ]) {
      expect(await waitForTestId(section, 5_000)).toBe(true);
    }
  });

  it('redirects /skills to /connections', async () => {
    stepLog('navigating to legacy /skills');
    await navigateViaHash('/skills');
    await browser.pause(500); // give the <Navigate replace /> a tick.
    const hash = await currentHash();
    expect(hash).toMatch(/^\/connections/);
  });

  it('deep-links /channels to /connections#channels and renders the channels section', async () => {
    stepLog('navigating to legacy /channels');
    await navigateViaHash('/channels');
    await browser.pause(500);
    const hash = await currentHash();
    expect(hash).toMatch(/^\/connections(?:#channels)?$/);
    expect(await waitForTestId('connections-section-channels', 5_000)).toBe(true);
  });

  it('creates a Generic HTTP connection through the modal', async () => {
    await navigateViaHash('/connections');
    expect(await waitForTestId('generic-http-add-button', 5_000)).toBe(true);

    stepLog('opening the Add HTTP connection modal');
    await clickByTestId('generic-http-add-button');
    expect(await waitForTestId('generic-http-edit-modal', 5_000)).toBe(true);

    stepLog('filling the modal form');
    await setInputByTestId('generic-http-modal-name', CONNECTION_NAME);
    await setInputByTestId('generic-http-modal-base-url', BASE_URL);
    // Leave auth-kind at its default ("none") — keeps the spec stable across
    // any future cred-input variations.

    stepLog('saving the connection');
    await clickByTestId('generic-http-modal-save');
    await waitForText(CONNECTION_NAME, 10_000);

    // Oracle: connections_list RPC must reflect the new row.
    const out = await callOpenhumanRpc('connections_list', {});
    const names: string[] = (out?.connections ?? []).map(
      (c: { display_name: string }) => c.display_name
    );
    expect(names).toContain(CONNECTION_NAME);
  });

  it('narrows visible cards via the search input', async () => {
    expect(await waitForTestId('connections-search-input', 5_000)).toBe(true);
    await setInputByTestId('connections-search-input', 'e2e-test');
    await browser.pause(400); // search applies on each keystroke (no debounce in P0-5).

    // The newly-created card should remain visible; built-in / MCP rows
    // with names that don't include "e2e-test" should be filtered out.
    expect(await textExists(CONNECTION_NAME)).toBe(true);
    expect(await textExists('Twilio')).toBe(false);

    // Clear the filter so the next test starts from a clean slate.
    await setInputByTestId('connections-search-input', '');
    await browser.pause(200);
  });

  it('deletes the Generic HTTP connection via the card affordance', async () => {
    // Find the delete button — testid is `generic-http-delete-<connection-id>`,
    // we use the connections_list RPC to recover the id without parsing DOM.
    const out = await callOpenhumanRpc('connections_list', {});
    const target = (out?.connections ?? []).find(
      (c: { display_name: string; ref: { type: string; connection_id?: string } }) =>
        c.display_name === CONNECTION_NAME && c.ref.type === 'generic_http'
    );
    expect(target, 'newly-created connection must be in the aggregator output').toBeTruthy();
    const id = (target as { ref: { connection_id: string } }).ref.connection_id;

    stepLog('deleting the connection', { id });
    // The card's Delete button confirms via window.confirm — stub it.
    await browser.execute(() => {
      window.confirm = () => true;
    });
    await clickByTestId(`generic-http-delete-${id}`);
    await browser.pause(500);

    // Oracle: subsequent connections_list must NOT contain the connection.
    const after = await callOpenhumanRpc('connections_list', {});
    const stillThere = (after?.connections ?? []).some(
      (c: { display_name: string }) => c.display_name === CONNECTION_NAME
    );
    expect(stillThere).toBe(false);
  });
});
