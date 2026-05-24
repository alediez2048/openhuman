// @ts-nocheck
/**
 * F-17 E2E — workflow memory loop end-to-end inside the real Tauri bundle.
 *
 * Verifies what the unit + integration tests (`workflows::executor_tests::memory_loop_*`)
 * pin against a stubbed agent — but inside the real CEF/Tauri/openhuman-core
 * binary against the actual `UnifiedMemory` SQLite backend wired through
 * `memory::global`. The unit tests prove the schema + recall rendering; this
 * spec proves the wiring survived the cargo-test → release-build → runtime-init
 * path.
 *
 * What it asserts:
 *   1. A fresh workflow has no prior chunks in `workflow/<id>` namespace.
 *   2. `workflows_run_now` (run 1) reaches a terminal status. Regardless
 *      of whether the agent succeeded or failed (the test workspace has
 *      no LLM provider configured, so failures are expected and fine —
 *      `persist_run_memory` writes the chunk either way per F-17 deliverable C).
 *   3. After run 1: exactly one chunk exists at namespace `workflow/<id>`
 *      with key `run:<uuid>`, content starts with `# Workflow run:`, and
 *      contains the canonical Markdown sections (`## Narrative`, `## Actual`,
 *      `## Entities`).
 *   4. After run 2: exactly two chunks exist, and the most recent chunk's
 *      `updated_at` is strictly newer than run 1's.
 *
 * What it does NOT assert (covered by the in-tree integration tests):
 *   - That run 2's composed prompt visibly contained run 1's recall line —
 *     that requires intercepting the agent's prompt, which the in-tree
 *     `memory_loop_stores_and_recalls_across_two_runs` test does via the
 *     test-stub override. The E2E spec's job is to prove the wire-up
 *     survived the boot path.
 *   - Confabulation regression (the in-tree integration test covers it).
 *   - Recall block rendering — covered by `memory.rs` unit tests.
 */
import { execSync } from 'node:child_process';

import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc } from '../helpers/core-rpc';
import { resetApp } from '../helpers/reset-app';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-workflows-memory-loop';

function stepLog(msg: string, ctx?: unknown): void {
  const stamp = new Date().toISOString();
  if (ctx === undefined) {
    console.log(`[WorkflowsMemoryLoopE2E][${stamp}] ${msg}`);
  } else {
    console.log(`[WorkflowsMemoryLoopE2E][${stamp}] ${msg}`, JSON.stringify(ctx));
  }
}

/** Discover the active per-user memory.db path. The bundled core stores
 *  per-user state at `<OPENHUMAN_WORKSPACE>/users/<user_id_hash>/workspace/`
 *  (not directly at `<OPENHUMAN_WORKSPACE>/memory/`). The user-id hash is
 *  derived from the auth deep-link's user id, so we discover it by:
 *    1. Reading `<OPENHUMAN_WORKSPACE>/active_user.toml` for `user_id = "…"`.
 *    2. Falling back to a recursive find for any `memory.db` under the
 *       workspace tmpdir (works even if active_user.toml format changes). */
function memoryDbPath(): string {
  const ws = process.env.OPENHUMAN_WORKSPACE;
  if (!ws) {
    throw new Error('OPENHUMAN_WORKSPACE not set — runner should export it');
  }
  // Prefer the active_user.toml hint.
  try {
    const toml = execSync(`cat '${ws}/active_user.toml' 2>/dev/null`, { encoding: 'utf8' });
    const m = toml.match(/^\s*user_id\s*=\s*"([^"]+)"/m);
    if (m && m[1]) {
      const path = `${ws}/users/${m[1]}/workspace/memory/memory.db`;
      const exists = execSync(`test -f '${path}' && echo yes || echo no`, {
        encoding: 'utf8',
      }).trim();
      if (exists === 'yes') return path;
    }
  } catch {
    /* fall through */
  }
  // Fallback: find any memory.db under the workspace.
  try {
    const found = execSync(`/usr/bin/find '${ws}' -name memory.db -type f 2>/dev/null | head -1`, {
      encoding: 'utf8',
    }).trim();
    if (found) return found;
  } catch {
    /* nothing */
  }
  return `${ws}/memory/memory.db`;
}

interface ChunkRow {
  namespace: string;
  key: string;
  bytes: number;
  updated_at: number;
}

function queryMemoryChunks(workflowId: string): ChunkRow[] {
  const sql = `SELECT namespace, key, length(content) AS bytes, updated_at FROM memory_docs WHERE namespace = 'workflow/${workflowId}' ORDER BY updated_at ASC;`;
  const dbPath = memoryDbPath();
  let raw: string;
  try {
    raw = execSync(`sqlite3 -separator '|' '${dbPath}' "${sql}"`, {
      encoding: 'utf8',
    }).trim();
  } catch (err) {
    // DB may not exist yet on a fresh workspace before the first run.
    stepLog(`sqlite3 query failed (likely DB not yet created): ${(err as Error).message}`);
    return [];
  }
  if (!raw) return [];
  return raw
    .split('\n')
    .map((line) => {
      const [namespace, key, bytes, updated_at] = line.split('|');
      return {
        namespace: namespace!,
        key: key!,
        bytes: Number(bytes),
        updated_at: Number(updated_at),
      };
    });
}

function fetchChunkContent(workflowId: string, runKey: string): string {
  const sql = `SELECT content FROM memory_docs WHERE namespace = 'workflow/${workflowId}' AND key = '${runKey}' LIMIT 1;`;
  const dbPath = memoryDbPath();
  return execSync(`sqlite3 '${dbPath}' "${sql}"`, { encoding: 'utf8' });
}

/** Unwrap the `RpcCallResult` envelope returned by `callOpenhumanRpc`,
 *  then strip any inner `RpcOutcome` `{ result, logs }` envelope from
 *  handlers that attached log messages via `RpcOutcome::single_log`. */
function unwrapRpc<T>(out: unknown): T {
  if (!out || typeof out !== 'object') {
    throw new Error(`callOpenhumanRpc returned non-object: ${JSON.stringify(out)}`);
  }
  const envelope = out as { ok: boolean; result?: unknown; error?: string };
  if (envelope.ok === false) {
    throw new Error(`RPC error: ${envelope.error ?? 'unknown'}`);
  }
  let value = envelope.result;
  if (value && typeof value === 'object') {
    const v = value as Record<string, unknown>;
    if ('result' in v && 'logs' in v) {
      value = v.result;
    }
  }
  return value as T;
}

async function createTrivialManualWorkflow(): Promise<string> {
  // Manual trigger + zero allowed_connections → health stays Ready
  // (no required Composio/Channel/Webview present to be missing).
  // The agent run itself may fail (no LLM configured in the test
  // workspace) but `persist_run_memory` writes the chunk regardless —
  // that's the whole point of best-effort post-run store.
  const request = {
    name: 'F-17 E2E manual',
    description: 'F-17 memory loop probe',
    trigger: { type: 'manual' },
    nodes: [
      {
        id: 'n1',
        kind: 'agent_prompt',
        config: {
          kind: 'agent_prompt',
          prompt: 'Say hello and stop. No tools needed.',
          allowed_connections: [],
          iteration_cap: 2,
        },
        position: null,
      },
    ],
    edges: [],
    origin: { type: 'user_chat' },
  };
  // Controller signature: handle_create reads `request` from params.
  // Method namespace is `openhuman.<namespace>_<function>`.
  const out = await callOpenhumanRpc('openhuman.workflows_create', { request });
  const wf = unwrapRpc<{ id: string }>(out);
  if (!wf?.id) throw new Error(`workflows_create did not return an id; got ${JSON.stringify(out)}`);
  return wf.id;
}

async function dispatchRunNow(workflowId: string): Promise<string> {
  // Controller signature: handle_run_now reads `workflow_id` + optional `initiator`.
  const out = await callOpenhumanRpc('openhuman.workflows_run_now', {
    workflow_id: workflowId,
    initiator: { type: 'user' },
  });
  // Return type is `RpcOutcome<RunId>` where RunId is a String.
  const runId = unwrapRpc<string>(out);
  if (typeof runId !== 'string' || !runId) {
    throw new Error(`workflows_run_now did not return a run id; got ${JSON.stringify(out)}`);
  }
  return runId;
}

async function waitForRunTerminal(workflowId: string, runId: string, timeoutMs = 20_000): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const out = await callOpenhumanRpc('openhuman.workflows_get_run', { run_id: runId });
    const wrapper = unwrapRpc<{ run?: { status: string } } | null>(out);
    const status = wrapper?.run?.status;
    if (status && status !== 'pending' && status !== 'running') {
      stepLog(`run ${runId} reached terminal status=${status}`);
      return status;
    }
    await browser.pause(400);
  }
  throw new Error(`run ${runId} of workflow ${workflowId} never reached terminal within ${timeoutMs}ms`);
}

describe('Workflows — F-17 memory loop (real binary)', () => {
  let workflowId: string;
  let run1Id: string;
  let run2Id: string;
  let run1ChunkUpdatedAt: number;

  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('creates a workflow with no prior memory chunks', async () => {
    workflowId = await createTrivialManualWorkflow();
    stepLog(`workflow id = ${workflowId}`);
    expect(workflowId).toMatch(/^[0-9a-f-]{36}$/);

    // Diagnostic: dump the workspace path + dir listing so we can see
    // where the core actually wrote storage when something goes wrong.
    const ws = process.env.OPENHUMAN_WORKSPACE ?? '(unset)';
    stepLog(`OPENHUMAN_WORKSPACE=${ws}`);
    stepLog(`resolved memoryDbPath()=${memoryDbPath()}`);

    const before = queryMemoryChunks(workflowId);
    expect(before.length).toBe(0);
  });

  it('run 1 lands a chunk in the workflow/<id> namespace (regardless of agent success)', async () => {
    run1Id = await dispatchRunNow(workflowId);
    const terminal = await waitForRunTerminal(workflowId, run1Id);
    stepLog(`run 1 terminal status = ${terminal}`);
    // Don't constrain on Succeeded vs Failed — the test workspace has no
    // LLM provider configured, so Failed is the realistic outcome.
    // persist_run_memory writes the chunk regardless per F-17 deliverable C.

    // Allow up to 5s for the post-run store to settle (it runs after the
    // step row write but before the run-row terminal write that we polled
    // on above; in practice it lands within hundreds of ms).
    let chunks: ChunkRow[] = [];
    const deadline = Date.now() + 5_000;
    while (Date.now() < deadline) {
      chunks = queryMemoryChunks(workflowId);
      if (chunks.length >= 1) break;
      await browser.pause(200);
    }
    expect(chunks.length).toBe(1);
    expect(chunks[0]!.namespace).toBe(`workflow/${workflowId}`);
    expect(chunks[0]!.key).toMatch(/^run:[0-9a-f-]{36}$/);
    expect(chunks[0]!.bytes).toBeGreaterThan(300);
    run1ChunkUpdatedAt = chunks[0]!.updated_at;

    // Inspect the chunk's content to confirm it's the canonical F-17
    // Markdown shape — not some other test artifact that accidentally
    // shares the namespace.
    const content = fetchChunkContent(workflowId, chunks[0]!.key);
    expect(content).toContain(`# Workflow run: ${workflowId}`);
    expect(content).toContain('## Narrative');
    expect(content).toContain('## Actual');
    expect(content).toContain('## Entities');
    expect(content).toContain('```json');
  });

  it('run 2 lands a second, strictly-newer chunk', async () => {
    // Ensure updated_at can actually move (UnifiedMemory uses f64 unix
    // seconds, sub-second granularity — sleep is paranoia, not strictly
    // needed).
    await browser.pause(1_100);

    run2Id = await dispatchRunNow(workflowId);
    expect(run2Id).not.toBe(run1Id);
    const terminal = await waitForRunTerminal(workflowId, run2Id);
    stepLog(`run 2 terminal status = ${terminal}`);

    let chunks: ChunkRow[] = [];
    const deadline = Date.now() + 5_000;
    while (Date.now() < deadline) {
      chunks = queryMemoryChunks(workflowId);
      if (chunks.length >= 2) break;
      await browser.pause(200);
    }
    expect(chunks.length).toBe(2);
    // Newest is last after ASC order — its updated_at must be > run 1's.
    const newest = chunks[chunks.length - 1]!;
    expect(newest.updated_at).toBeGreaterThan(run1ChunkUpdatedAt);
    expect(newest.key).toMatch(/^run:[0-9a-f-]{36}$/);
    expect(newest.key).not.toBe(chunks[0]!.key);
  });
});
