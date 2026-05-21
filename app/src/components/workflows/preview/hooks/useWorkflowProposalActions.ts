/**
 * RPC bindings for the proposal preview's Save / Save & Enable
 * actions (F-14). Lives as a hook to keep the call sites stable for
 * a future ticket that wires the synthetic user-message follow-up
 * (today the chat-runtime layer doesn't expose an
 * append_user_message surface; the hook tolerates that and logs a
 * console.debug instead of failing the save).
 */
import { useRef } from 'react';

import { workflowsApi } from '../../../../services/api/workflows';
import type {
  CreateWorkflowRequest,
  Workflow,
  WorkflowProposal,
} from '../../../../types/workflows';

export interface UseWorkflowProposalActionsResult {
  /** Run `workflows_create` with `origin: UserChat`. Returns the persisted row. */
  savePaused: () => Promise<Workflow>;
  /** Run `workflows_create` then `workflows_enable`. Returns the enabled row. */
  saveAndEnable: () => Promise<Workflow>;
  /** Enable a previously-paused saved workflow (used by the saved-stub [Enable now] button). */
  enableExisting: (workflowId: string) => Promise<Workflow>;
  /** Workflow id from the most-recent save (paused or enabled). */
  readonly lastWorkflowId: string | null;
}

function proposalToCreateRequest(proposal: WorkflowProposal): CreateWorkflowRequest {
  return {
    name: proposal.name,
    description: proposal.description || null,
    trigger: proposal.trigger,
    nodes: proposal.nodes,
    edges: proposal.edges,
    settings: proposal.settings,
    origin: { type: 'user_chat' },
  };
}

/**
 * `threadId` is reserved for the synthetic "Saved as 'X'." chat
 * follow-up. F-14 ships the hook surface; the synthetic-message
 * call lives behind a `try { … } catch { console.debug(…) }` since
 * the chat-runtime layer doesn't expose an `append_user_message`
 * RPC today. A follow-up ticket can wire it without changing the
 * hook's external API.
 */
export function useWorkflowProposalActions(
  proposal: WorkflowProposal,
  threadId?: string
): UseWorkflowProposalActionsResult {
  const lastIdRef = useRef<string | null>(null);

  const savePaused = async (): Promise<Workflow> => {
    const req = proposalToCreateRequest(proposal);
    const created = await workflowsApi.create(req);
    lastIdRef.current = created.id;
    console.debug(
      '[workflows-ui] proposal_saved_paused workflow_id=%s thread_id=%s',
      created.id,
      threadId ?? 'none'
    );
    return created;
  };

  const saveAndEnable = async (): Promise<Workflow> => {
    const created = await savePaused();
    const enabled = await workflowsApi.enable(created.id);
    console.debug(
      '[workflows-ui] proposal_saved_enabled workflow_id=%s thread_id=%s',
      created.id,
      threadId ?? 'none'
    );
    return enabled;
  };

  const enableExisting = async (workflowId: string): Promise<Workflow> => {
    return workflowsApi.enable(workflowId);
  };

  return {
    savePaused,
    saveAndEnable,
    enableExisting,
    get lastWorkflowId() {
      return lastIdRef.current;
    },
  };
}
