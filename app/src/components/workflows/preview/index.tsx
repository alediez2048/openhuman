/**
 * Public surface for the F-14 workflow preview rich-message
 * components. Re-exports the four cards + a `renderWorkflowPreview`
 * registry helper the chat-runtime layer will call once the agent
 * message protocol carries structured payloads (deferred; the
 * chat-runtime is markdown-only today — F-15's hero E2E lands the
 * wire-up by extending the agent message segment shape).
 *
 * The registry is keyed by a single discriminator
 * (`preview.kind`) so callers handle dispatch in one place:
 *
 * ```ts
 * const node = renderWorkflowPreview({ kind: 'proposal', proposal, missingConnections });
 * ```
 *
 * Returning `null` for an unknown kind keeps the chat thread from
 * crashing on a forward-compat payload a future Phase 2 tool might
 * emit.
 */
import type { ReactElement } from 'react';

import type { ConnectionRef } from '../../../types/connections';
import type {
  WorkflowDeletePreview as DeletePayload,
  WorkflowEditProposal,
  WorkflowProposal,
  WorkflowStateProposal,
} from '../../../types/workflows';
import { WorkflowDeletePreview } from './WorkflowDeletePreview';
import { WorkflowEditPreview } from './WorkflowEditPreview';
import { WorkflowProposalPreview } from './WorkflowProposalPreview';
import { WorkflowStatePreview } from './WorkflowStatePreview';

export { WorkflowProposalPreview } from './WorkflowProposalPreview';
export { WorkflowEditPreview } from './WorkflowEditPreview';
export { WorkflowDeletePreview } from './WorkflowDeletePreview';
export { WorkflowStatePreview } from './WorkflowStatePreview';
export { useCronHumanizer } from './hooks/useCronHumanizer';
export { useConnectionMeta, metaForRef } from './hooks/useConnectionMeta';

/**
 * Discriminated union the chat-runtime layer dispatches on. Each
 * entry carries the matching server payload + any UI-only context
 * the renderer needs to enrich the card (e.g. computed
 * `missingConnections` from the live aggregator snapshot).
 */
export type WorkflowPreviewPayload =
  | {
      kind: 'proposal';
      proposal: WorkflowProposal;
      threadId?: string;
      missingConnections?: ConnectionRef[];
    }
  | { kind: 'edit'; proposal: WorkflowEditProposal }
  | { kind: 'delete'; preview: DeletePayload }
  | { kind: 'state'; proposal: WorkflowStateProposal; workflowName?: string };

/**
 * Render the right preview component for a tagged payload. Returns
 * `null` for an unknown kind so the chat thread degrades
 * gracefully on a forward-compat payload.
 *
 * The chat-runtime integration is deferred to F-15: the agent
 * message segment shape today is markdown only, so wiring this
 * registry into `AgentMessageBubble` requires a parallel ticket
 * that extends the agent → frontend message protocol. F-14
 * ships the components + registry so F-15 can swap them in
 * body-only.
 */
export function renderWorkflowPreview(payload: WorkflowPreviewPayload): ReactElement | null {
  switch (payload.kind) {
    case 'proposal':
      return (
        <WorkflowProposalPreview
          proposal={payload.proposal}
          threadId={payload.threadId}
          missingConnections={payload.missingConnections}
        />
      );
    case 'edit':
      return <WorkflowEditPreview proposal={payload.proposal} />;
    case 'delete':
      return <WorkflowDeletePreview preview={payload.preview} />;
    case 'state':
      return (
        <WorkflowStatePreview proposal={payload.proposal} workflowName={payload.workflowName} />
      );
    default:
      return null;
  }
}
