import Markdown from 'react-markdown';

import { OPENHUMAN_LINK_EVENT } from '../../../components/OpenhumanLinkModal';
import {
  renderWorkflowPreview,
  type WorkflowPreviewPayload,
} from '../../../components/workflows/preview';
import { parseMarkdownTable } from '../../../utils/agentMessageBubbles';
import { openUrl } from '../../../utils/openUrl';
import {
  type AgentBubblePosition,
  getAgentBubbleChrome,
  isAllowedExternalHref,
  parseBubbleSegments,
} from '../utils/format';

/**
 * Renders the matching F-14 workflow-preview component for an
 * inline `<workflow-preview kind="..." data='{...}'>` tag emitted
 * by the F-12 propose-tool surface. Parse failures show a small
 * fallback so the chat thread doesn't crash on a malformed
 * payload (which can happen during early LLM drafts).
 */
function WorkflowPreviewSlot({
  previewKind,
  data,
}: {
  previewKind: 'proposal' | 'edit' | 'delete' | 'state';
  data: string;
}) {
  let parsed: unknown;
  try {
    parsed = JSON.parse(data);
  } catch (err) {
    console.error('[chat-runtime] workflow-preview data parse failed', err, data);
    return (
      <div className="rounded-xl border border-coral-200 bg-coral-50 px-3 py-2 text-xs text-coral-700">
        Couldn’t render the workflow preview (invalid payload).
      </div>
    );
  }
  let payload: WorkflowPreviewPayload | null = null;
  try {
    if (previewKind === 'proposal') {
      payload = { kind: 'proposal', proposal: parsed as never };
    } else if (previewKind === 'edit') {
      payload = { kind: 'edit', proposal: parsed as never };
    } else if (previewKind === 'delete') {
      payload = { kind: 'delete', preview: parsed as never };
    } else if (previewKind === 'state') {
      payload = { kind: 'state', proposal: parsed as never };
    }
  } catch (err) {
    console.error('[chat-runtime] workflow-preview shape mismatch', err);
  }
  const node = payload ? renderWorkflowPreview(payload) : null;
  return (
    node ?? (
      <div className="rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800">
        Workflow preview type “{previewKind}” isn’t supported yet.
      </div>
    )
  );
}

/**
 * Pill rendered below an agent bubble for each
 * `<openhuman-link path="...">label</openhuman-link>` tag the agent
 * emits. Click dispatches an `OPENHUMAN_LINK_EVENT` window event that
 * `OpenhumanLinkModal` listens for, so the chat stays in view.
 */
function OpenhumanLinkPill({ path, label }: { path: string; label: string }) {
  return (
    <button
      type="button"
      onClick={() =>
        window.dispatchEvent(new CustomEvent(OPENHUMAN_LINK_EVENT, { detail: { path } }))
      }
      className="inline-flex items-center gap-1 rounded-full border border-primary-200 bg-primary-50 px-3 py-1 text-xs font-medium text-primary-700 transition-colors hover:bg-primary-100">
      {label}
      <svg className="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor">
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M5 12h14M13 6l6 6-6 6"
        />
      </svg>
    </button>
  );
}

export function BubbleMarkdown({
  content,
  tone = 'agent',
}: {
  content: string;
  tone?: 'agent' | 'user';
}) {
  const proseTone =
    tone === 'user'
      ? 'prose-invert prose-p:text-white prose-li:text-white prose-a:text-white prose-code:text-white prose-strong:text-white prose-headings:text-white [&_li::marker]:text-white/85'
      : 'dark:prose-invert prose-a:text-primary-500 prose-code:text-primary-700 dark:prose-code:text-primary-300 prose-headings:text-sm [&_li::marker]:text-stone-700 dark:[&_li::marker]:text-neutral-300';

  return (
    <div
      className={`text-sm prose prose-sm max-w-none prose-p:my-1 prose-pre:my-2 prose-pre:rounded-lg prose-code:text-xs prose-headings:font-semibold prose-ul:my-0 prose-ol:my-0 prose-li:my-0 ${proseTone} ${
        tone === 'user' ? 'prose-pre:bg-white/10' : 'prose-pre:bg-stone-300/50'
      } [&_ul]:my-0 [&_ol]:my-0 [&_ul]:pl-0 [&_ol]:pl-0 [&_ul]:list-inside [&_ol]:list-inside [&_li]:my-0 [&_li]:pl-0 [&_li_p]:inline [&_li_p]:m-0`}>
      <Markdown
        components={{
          a: ({ href, children }) => (
            <a
              href={href}
              onClick={e => {
                e.preventDefault();
                if (!href || !isAllowedExternalHref(href)) return;
                void openUrl(href).catch(() => {
                  // Ignore launcher errors from OS URL handler failures.
                });
              }}
              className="cursor-pointer underline">
              {children}
            </a>
          ),
        }}>
        {content}
      </Markdown>
    </div>
  );
}

export function TableCellMarkdown({ content }: { content: string }) {
  return (
    <div className="prose prose-sm dark:prose-invert max-w-none text-sm text-stone-700 dark:text-neutral-200 prose-p:my-0 prose-ul:my-0 prose-ol:my-0 prose-li:my-0 prose-code:text-xs prose-code:text-primary-700 dark:prose-code:text-primary-300 prose-a:text-primary-500 prose-strong:text-stone-900 dark:prose-strong:text-neutral-100 prose-headings:text-sm prose-headings:font-semibold [&_li::marker]:text-stone-700 dark:[&_li::marker]:text-neutral-300 [&_ul]:my-0 [&_ol]:my-0 [&_ul]:pl-0 [&_ol]:pl-0 [&_ul]:list-inside [&_ol]:list-inside [&_li]:pl-0 [&_li_p]:inline [&_li_p]:m-0">
      <Markdown
        components={{
          a: ({ href, children }) => (
            <a
              href={href}
              onClick={e => {
                e.preventDefault();
                if (!href || !isAllowedExternalHref(href)) return;
                void openUrl(href).catch(() => {
                  // Ignore launcher errors from OS URL handler failures.
                });
              }}
              className="cursor-pointer underline">
              {children}
            </a>
          ),
        }}>
        {content}
      </Markdown>
    </div>
  );
}

export function AgentMessageBubble({
  content,
  position = 'single',
}: {
  content: string;
  position?: AgentBubblePosition;
}) {
  const segments = parseBubbleSegments(content);
  const textContent = segments
    .filter(s => s.kind === 'text')
    .map(s => s.text)
    .join('')
    .trim();
  const linkSegments = segments.filter(
    (s): s is Extract<typeof s, { kind: 'link' }> => s.kind === 'link'
  );
  const workflowPreviews = segments.filter(
    (s): s is Extract<typeof s, { kind: 'workflow_preview' }> => s.kind === 'workflow_preview'
  );

  const table = parseMarkdownTable(textContent);
  const bubbleChrome = getAgentBubbleChrome(position);

  if (table) {
    return (
      <div
        className={`w-full max-w-full overflow-hidden border border-stone-200 dark:border-neutral-800 bg-white/90 dark:bg-neutral-900/90 shadow-sm ${bubbleChrome}`}>
        <div className="overflow-x-auto">
          <table className="w-max min-w-full border-collapse text-left text-sm text-stone-800 dark:text-neutral-100">
            <thead className="bg-stone-100 dark:bg-neutral-800/90">
              <tr>
                {table.headers.map(header => (
                  <th
                    key={header}
                    className="max-w-[25vw] border-b border-stone-200 dark:border-neutral-800 px-4 py-2.5 text-xs font-semibold uppercase tracking-[0.08em] text-stone-500 dark:text-neutral-400">
                    {header}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {table.rows.map((row, rowIndex) => (
                <tr
                  key={`${rowIndex}:${row.join('|')}`}
                  className="odd:bg-white dark:odd:bg-neutral-900 even:bg-stone-50 dark:even:bg-neutral-800/60">
                  {row.map((cell, cellIndex) => (
                    <td
                      key={`${rowIndex}:${cellIndex}:${cell}`}
                      className="max-w-[25vw] border-t border-stone-200 dark:border-neutral-800 px-4 py-3 align-top text-sm text-stone-700 dark:text-neutral-200">
                      <TableCellMarkdown content={cell} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    );
  }

  return (
    <>
      {textContent && (
        <div
          className={`bg-stone-200 dark:bg-neutral-800/80 px-4 py-2.5 text-stone-900 dark:text-neutral-100 ${bubbleChrome}`}>
          <BubbleMarkdown content={textContent} />
        </div>
      )}
      {linkSegments.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-2">
          {linkSegments.map((segment, idx) => (
            <OpenhumanLinkPill
              key={`pill-${idx}-${segment.path}`}
              path={segment.path}
              label={segment.label}
            />
          ))}
        </div>
      )}
      {workflowPreviews.length > 0 && (
        <div className="mt-2 flex flex-col gap-2">
          {workflowPreviews.map((segment, idx) => (
            <WorkflowPreviewSlot
              key={`wf-preview-${idx}-${segment.previewKind}`}
              previewKind={segment.previewKind}
              data={segment.data}
            />
          ))}
        </div>
      )}
    </>
  );
}
