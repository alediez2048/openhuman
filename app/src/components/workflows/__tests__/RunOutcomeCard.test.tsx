/**
 * T-2 (Phase 2.5 Trust UX) — unit tests for `<RunOutcomeCard>`.
 *
 * Pins the rendering contract for each `SideEffectKind` variant + the
 * failure / running / zero-receipts-warning branches so future renderer
 * changes can't silently break the trust UX.
 */
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { DeliveryReceipt, Run, RunStep, Workflow } from '../../../types/workflows';
import RunOutcomeCard from '../RunOutcomeCard';

const invokeMock = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invokeMock(...args) }));

vi.mock('../../../lib/i18n/I18nContext', () => ({
  useT: () => ({
    t: (key: string) => {
      // Surface the keys in the rendered DOM so assertions can target
      // them deterministically without depending on copy.
      switch (key) {
        case 'workflows.outcome.email_sent_to':
          return 'Sent email to {recipient}';
        case 'workflows.outcome.message_posted_in':
          return 'Posted message in {provider} ({recipient})';
        case 'workflows.outcome.file_created_in':
          return 'Created {title} in {provider}';
        case 'workflows.outcome.record_created_in':
          return 'Created record {name} in {provider}';
        case 'workflows.outcome.record_updated_in':
          return 'Updated record {name} in {provider}';
        case 'workflows.outcome.calendar_event_created':
          return 'Created calendar event {title}';
        case 'workflows.outcome.issue_created_in':
          return 'Created issue {title} in {provider}';
        case 'workflows.outcome.social_post_created':
          return 'Posted on {provider}: "{snippet}"';
        case 'workflows.outcome.open_issue':
          return 'Open';
        case 'workflows.outcome.open_post':
          return 'Open';
        case 'workflows.outcome.other_action':
          return '{verb} via {tool}';
        case 'workflows.outcome.open_in_gmail':
          return 'Open in Gmail';
        case 'workflows.outcome.show_agent_notes':
          return "Show agent's notes";
        case 'workflows.outcome.hide_agent_notes':
          return "Hide agent's notes";
        case 'workflows.outcome.zero_receipts_warning':
          return 'No observable side effects';
        case 'workflows.outcome.what_happened':
          return 'What this run did';
        case 'workflows.outcome.why_failed':
          return 'Why this run failed';
        case 'workflows.outcome.succeeded':
          return 'Succeeded';
        case 'workflows.outcome.failed':
          return 'Failed';
        // T-4 failure-mode renderer keys
        case 'workflows.failure.agent_narrated':
          return 'The agent described what it would do ({chars} chars of text) but never actually emitted a tool call.';
        case 'workflows.failure.agent_narrated_hint':
          return 'Common cause: model stuck in chain-of-thought. Try `agentic-v1`.';
        case 'workflows.failure.model_unavailable':
          return 'Model `{model}` isn’t available in your account.';
        case 'workflows.failure.model_unavailable_hint':
          return 'Edit the workflow → set model_tier to one of: {tiers}.';
        case 'workflows.failure.connection_expired':
          return 'Your {provider} connection expired or was revoked.';
        case 'workflows.failure.connection_expired_hint':
          return 'Reconnect {provider} in Settings → Connections.';
        case 'workflows.failure.composio_rejected':
          return '`{tool}` was rejected by the connected provider: {detail}';
        case 'workflows.failure.composio_rejected_hint':
          return 'The provider’s reason is verbatim above.';
        case 'workflows.failure.llm_auth_failed':
          return 'Your {provider} API key was rejected.';
        case 'workflows.failure.llm_auth_failed_hint':
          return 'Update the key in Settings → Providers.';
        case 'workflows.failure.tool_slug_invalid':
          return 'The agent tried to use an invalid tool name (`{slug}`).';
        case 'workflows.failure.tool_slug_invalid_hint':
          return 'Retry the run.';
        case 'workflows.failure.unknown_detail':
          return '{detail}';
        default:
          return key;
      }
    },
  }),
}));

function runOf(overrides: Partial<Run> = {}): Run {
  return {
    id: 'run-1',
    workflow_id: 'wf-1',
    trigger_source: { type: 'manual', initiator: 'user' },
    status: 'succeeded',
    started_at: new Date(Date.now() - 60_000).toISOString(),
    completed_at: new Date().toISOString(),
    cancelled: false,
    ...overrides,
  };
}

function stepOf(overrides: Partial<RunStep> = {}): RunStep {
  return {
    id: 'step-1',
    run_id: 'run-1',
    node_id: 'n1',
    status: 'succeeded',
    started_at: new Date().toISOString(),
    completed_at: new Date().toISOString(),
    output_json: null,
    error: null,
    delivery_receipts: [],
    ...overrides,
  };
}

function receiptOf(overrides: Partial<DeliveryReceipt> = {}): DeliveryReceipt {
  return {
    tool: 'GMAIL_SEND_EMAIL',
    side_effect_kind: { kind: 'email_sent' },
    recipient: 'alediez2408@gmail.com',
    message_id: 'mid-abc',
    link: 'https://mail.google.com/mail/u/0/#sent/mid-abc',
    at: new Date().toISOString(),
    ...overrides,
  };
}

function workflowWith(allowedCount: number): Workflow {
  const allowed_connections = Array.from({ length: allowedCount }, (_, i) => ({
    type: 'composio' as const,
    toolkit_id: `tk${i}`,
  }));
  return {
    id: 'wf-1',
    schema_version: 1,
    name: 'test',
    description: null,
    enabled: true,
    origin: { type: 'user_chat' },
    health: { type: 'ready' },
    trigger: { type: 'manual' },
    nodes: [
      {
        id: 'n1',
        kind: 'agent_prompt',
        config: {
          kind: 'agent_prompt',
          prompt: 'do a thing',
          allowed_connections,
          iteration_cap: 10,
        },
      },
    ],
    edges: [],
    settings: { timeout_secs: 300, on_error: 'halt' },
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    last_run_at: null,
  };
}

describe('<RunOutcomeCard>', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it('renders one EmailSent receipt with a clickable deep link', () => {
    render(<RunOutcomeCard run={runOf()} steps={[stepOf({ delivery_receipts: [receiptOf()] })]} />);
    expect(screen.getByText('Sent email to alediez2408@gmail.com')).toBeInTheDocument();
    const openButton = screen.getByRole('button', { name: /Open in Gmail/i });
    expect(openButton).toBeInTheDocument();
    fireEvent.click(openButton);
    expect(invokeMock).toHaveBeenCalledWith('plugin:opener|open_url', {
      url: 'https://mail.google.com/mail/u/0/#sent/mid-abc',
    });
  });

  it('renders one MessagePosted receipt with the channel surfaced', () => {
    render(
      <RunOutcomeCard
        run={runOf()}
        steps={[
          stepOf({
            delivery_receipts: [
              receiptOf({
                tool: 'SLACK_SEND_MESSAGE',
                side_effect_kind: { kind: 'message_posted', provider: 'slack' },
                recipient: '#general',
                link: null,
              }),
            ],
          }),
        ]}
      />
    );
    expect(screen.getByText(/Posted message in Slack \(#general\)/)).toBeInTheDocument();
    // No link → no Open button
    expect(screen.queryByRole('button', { name: /^Open/ })).not.toBeInTheDocument();
  });

  it('renders calendar event creation with title', () => {
    render(
      <RunOutcomeCard
        run={runOf()}
        steps={[
          stepOf({
            delivery_receipts: [
              receiptOf({
                tool: 'GOOGLECALENDAR_CREATE_EVENT',
                side_effect_kind: { kind: 'calendar_event_created' },
                recipient: 'Standup',
                link: 'https://calendar.google.com/event?eid=x',
              }),
            ],
          }),
        ]}
      />
    );
    expect(screen.getByText('Created calendar event Standup')).toBeInTheDocument();
  });

  it('renders Other-kind fall-through with verb + tool name', () => {
    render(
      <RunOutcomeCard
        run={runOf()}
        steps={[
          stepOf({
            delivery_receipts: [
              receiptOf({
                tool: 'WIDGETS_SEND_FOO',
                side_effect_kind: { kind: 'other', verb: 'Sent' },
                recipient: null,
                link: null,
                message_id: null,
              }),
            ],
          }),
        ]}
      />
    );
    expect(screen.getByText('Sent via WIDGETS_SEND_FOO')).toBeInTheDocument();
  });

  it('renders the zero-receipts warning when Succeeded + action connections + no receipts', () => {
    render(
      <RunOutcomeCard
        run={runOf({ status: 'succeeded' })}
        steps={[stepOf({ delivery_receipts: [] })]}
        workflow={workflowWith(2)}
      />
    );
    expect(screen.getByText(/No observable side effects/)).toBeInTheDocument();
  });

  it('does NOT render the zero-receipts warning when workflow has no action connections', () => {
    render(
      <RunOutcomeCard
        run={runOf({ status: 'succeeded' })}
        steps={[stepOf({ delivery_receipts: [] })]}
        workflow={workflowWith(0)}
      />
    );
    expect(screen.queryByText(/No observable side effects/)).not.toBeInTheDocument();
  });

  it('renders the failure section with the step error when status is failed', () => {
    render(
      <RunOutcomeCard
        run={runOf({ status: 'failed', error: 'tool blew up' })}
        steps={[stepOf({ status: 'failed', error: 'tool blew up' })]}
      />
    );
    expect(screen.getByText('Why this run failed')).toBeInTheDocument();
    expect(screen.getByTestId('run-outcome-failure')).toHaveTextContent('tool blew up');
  });

  it('collapses agent narrative behind a disclosure', () => {
    render(
      <RunOutcomeCard
        run={runOf()}
        steps={[
          stepOf({
            output_json: JSON.stringify({ text: 'agent rambled about something' }),
            delivery_receipts: [receiptOf()],
          }),
        ]}
      />
    );
    // Narrative is hidden by default
    expect(screen.queryByTestId('run-outcome-narrative')).not.toBeInTheDocument();
    // Expand via the disclosure button
    fireEvent.click(screen.getByRole('button', { name: /Show agent's notes/i }));
    expect(screen.getByTestId('run-outcome-narrative')).toHaveTextContent(
      'agent rambled about something'
    );
  });

  it('renders an IssueCreated receipt with the 🎫 icon shape', () => {
    render(
      <RunOutcomeCard
        run={runOf()}
        steps={[
          stepOf({
            delivery_receipts: [
              receiptOf({
                tool: 'LINEAR_CREATE_ISSUE',
                side_effect_kind: { kind: 'issue_created', provider: 'linear' },
                recipient: 'Fix login bug',
                message_id: 'ENG-42',
                link: 'https://linear.app/acme/issue/ENG-42',
              }),
            ],
          }),
        ]}
      />
    );
    expect(screen.getByText('Created issue Fix login bug in Linear')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Open/i })).toBeInTheDocument();
  });

  it('renders a SocialPostCreated receipt with the 📢 shape', () => {
    render(
      <RunOutcomeCard
        run={runOf()}
        steps={[
          stepOf({
            delivery_receipts: [
              receiptOf({
                tool: 'LINKEDIN_CREATE_LINKED_IN_POST',
                side_effect_kind: { kind: 'social_post_created', provider: 'linkedin' },
                recipient: 'Excited to ship Trust UX in OpenHuman…',
                message_id: 'urn:li:share:abc',
                link: null,
              }),
            ],
          }),
        ]}
      />
    );
    expect(
      screen.getByText(/Posted on Linkedin: "Excited to ship Trust UX in OpenHuman…"/)
    ).toBeInTheDocument();
  });

  // ── T-4 (Phase 2.5 Trust UX): structured failure-mode rendering ──

  it('renders structured AgentNarratedWithoutActing failure with fix-it hint', () => {
    render(
      <RunOutcomeCard
        run={runOf({
          status: 'failed',
          error: 'agent narrated next-action intent in 340 chars',
          failure_reason: { kind: 'agent_narrated_without_acting', narrative_chars: 340 },
        })}
        steps={[stepOf({ status: 'failed', error: 'agent narrated' })]}
      />
    );
    expect(screen.getByTestId('run-outcome-failure')).toHaveTextContent(
      /described what it would do \(340 chars/
    );
    // Fix-it hint surfaces the agentic-v1 suggestion
    expect(screen.getByText(/agentic-v1/)).toBeInTheDocument();
  });

  it('renders structured ModelUnavailable failure with valid_tiers in the hint', () => {
    render(
      <RunOutcomeCard
        run={runOf({
          status: 'failed',
          error: "Model 'claude-opus-4-7' is not available.",
          failure_reason: {
            kind: 'model_unavailable',
            model_tried: 'claude-opus-4-7',
            valid_tiers: ['agentic-v1', 'reasoning-v1', 'chat-v1'],
          },
        })}
        steps={[stepOf({ status: 'failed' })]}
      />
    );
    expect(screen.getByTestId('run-outcome-failure')).toHaveTextContent(
      /Model `claude-opus-4-7` isn.t available/
    );
    expect(screen.getByText(/agentic-v1, reasoning-v1, chat-v1/)).toBeInTheDocument();
  });

  it('renders structured ConnectionExpired failure with provider in hint', () => {
    render(
      <RunOutcomeCard
        run={runOf({
          status: 'failed',
          error: 'gmail auth_failed',
          failure_reason: { kind: 'connection_expired', provider: 'gmail' },
        })}
        steps={[stepOf({ status: 'failed' })]}
      />
    );
    expect(screen.getByTestId('run-outcome-failure')).toHaveTextContent(
      /Your Gmail connection expired/
    );
    expect(screen.getByText(/Reconnect Gmail in Settings/)).toBeInTheDocument();
  });

  it('falls back to raw error string when failure_reason is null', () => {
    // Pre-T-4 rows (or transient cases where classification didn't
    // run) still surface the raw error rather than a blank section.
    render(
      <RunOutcomeCard
        run={runOf({
          status: 'failed',
          error: 'some legacy opaque error string',
          failure_reason: null,
        })}
        steps={[stepOf({ status: 'failed', error: 'some legacy opaque error string' })]}
      />
    );
    expect(screen.getByTestId('run-outcome-failure')).toHaveTextContent(
      /some legacy opaque error string/
    );
  });

  it('renders multiple receipts in order across steps', () => {
    render(
      <RunOutcomeCard
        run={runOf()}
        steps={[
          stepOf({
            id: 'step-1',
            delivery_receipts: [
              receiptOf({ tool: 'GMAIL_SEND_EMAIL' }),
              receiptOf({
                tool: 'SLACK_SEND_MESSAGE',
                side_effect_kind: { kind: 'message_posted', provider: 'slack' },
                recipient: '#general',
                link: null,
              }),
            ],
          }),
        ]}
      />
    );
    const rows = screen.getAllByTestId('receipt-row');
    expect(rows).toHaveLength(2);
  });
});
