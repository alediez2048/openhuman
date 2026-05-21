/**
 * Inspectable details panel rendered when the user expands the
 * "Show details" disclosure. Four sections (radio-behavior — one
 * open at a time): Rationale, Agent prompt, Required connections,
 * Settings. The `Rationale` section auto-expands when
 * `confidence === 'low'` (the design spec's signal that the agent
 * is hedging and the user benefits from seeing the reasoning
 * up-front).
 */
import { useEffect, useState } from 'react';

import { useT } from '../../../../lib/i18n/I18nContext';
import type { WorkflowProposal } from '../../../../types/workflows';
import { useConnectionMeta } from '../hooks/useConnectionMeta';

type SectionKey = 'rationale' | 'prompt' | 'connections' | 'settings';

interface Props {
  proposal: WorkflowProposal;
}

export function DetailsPanel({ proposal }: Props) {
  const { t } = useT();
  const [open, setOpen] = useState<SectionKey | null>(
    proposal.confidence === 'low' ? 'rationale' : null
  );
  useEffect(() => {
    if (proposal.confidence === 'low') {
      setOpen('rationale');
    }
  }, [proposal.confidence]);

  const node = proposal.nodes[0];
  const cfg = node?.config && node.config.kind === 'agent_prompt' ? node.config : null;
  const prompt = cfg?.prompt ?? '';
  const promptFirstLine = prompt.split('\n')[0] ?? '';

  const sections: Array<{ key: SectionKey; label: string; badge?: string; body: React.ReactNode }> =
    [
      {
        key: 'rationale',
        label: t('workflows.preview.rationale'),
        badge: proposal.rationale.length > 0 ? String(proposal.rationale.length) : undefined,
        body: <RationaleBullets rationale={proposal.rationale} />,
      },
      {
        key: 'prompt',
        label: t('workflows.preview.agent_prompt'),
        body: <PromptViewer text={prompt} />,
      },
      {
        key: 'connections',
        label: t('workflows.preview.required_connections'),
        badge:
          proposal.required_connections.length > 0
            ? String(proposal.required_connections.length)
            : undefined,
        body: <ConnectionsTable proposal={proposal} />,
      },
      {
        key: 'settings',
        label: t('workflows.preview.settings'),
        body: (
          <SettingsTable
            timeoutSecs={proposal.settings.timeout_secs}
            onError={proposal.settings.on_error}
            iterationCap={cfg?.iteration_cap}
            modelTier={cfg?.model_tier ?? null}
          />
        ),
      },
    ];

  return (
    <div className="mt-3 border-t border-stone-100 dark:border-neutral-700 pt-3">
      {sections.map(section => {
        const expanded = open === section.key;
        return (
          <div
            key={section.key}
            className="border-b border-stone-100 dark:border-neutral-700 last:border-b-0">
            <button
              type="button"
              onClick={() => setOpen(expanded ? null : section.key)}
              aria-expanded={expanded}
              aria-controls={`details-${section.key}`}
              className="w-full flex items-center justify-between py-2 text-left text-xs font-medium text-stone-700 dark:text-neutral-200 hover:text-primary-700 dark:hover:text-primary-300">
              <span className="flex items-center gap-2">
                <span aria-hidden className="text-stone-400 dark:text-neutral-500">
                  {expanded ? '⌃' : '▸'}
                </span>
                {section.label}
                {section.key === 'prompt' && promptFirstLine && !expanded && (
                  <span className="text-stone-400 dark:text-neutral-500 truncate max-w-[200px] font-normal">
                    {promptFirstLine}
                  </span>
                )}
              </span>
              {section.badge && (
                <span className="text-[11px] text-stone-500 dark:text-neutral-400 font-normal">
                  ({section.badge})
                </span>
              )}
            </button>
            {expanded && (
              <div id={`details-${section.key}`} className="pb-3 pl-5">
                {section.body}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ── Sub-sections ──────────────────────────────────────────────────────

function RationaleBullets({ rationale }: { rationale: string[] }) {
  if (rationale.length === 0) {
    return (
      <p className="text-xs text-stone-500 dark:text-neutral-400 italic">No rationale provided.</p>
    );
  }
  return (
    <ul className="text-xs text-stone-700 dark:text-neutral-300 list-disc list-inside space-y-1">
      {rationale.map((bullet, i) => (
        <li key={i}>{bullet}</li>
      ))}
    </ul>
  );
}

function PromptViewer({ text }: { text: string }) {
  const { t } = useT();
  const [copied, setCopied] = useState(false);
  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard API can fail under strict browser policies; silent
      // fallback keeps the panel usable (the text is already on
      // screen for the user to copy manually).
    }
  };
  return (
    <div className="relative">
      <pre
        className="font-mono text-[11px] bg-stone-50 dark:bg-neutral-800 text-stone-700 dark:text-neutral-200 p-2 rounded-md max-h-64 overflow-auto whitespace-pre-wrap break-words"
        aria-label="Agent prompt">
        {text}
      </pre>
      <button
        type="button"
        onClick={onCopy}
        className="absolute top-1 right-1 text-[10px] text-stone-500 dark:text-neutral-400 hover:text-stone-700 dark:hover:text-neutral-200 bg-white/80 dark:bg-neutral-900/80 px-1.5 py-0.5 rounded">
        {copied ? t('workflows.preview.copied') : t('workflows.preview.copy_prompt')}
      </button>
    </div>
  );
}

function ConnectionsTable({ proposal }: { proposal: WorkflowProposal }) {
  const { t } = useT();
  const metas = useConnectionMeta(proposal.required_connections);
  if (metas.length === 0) {
    return (
      <p className="text-xs text-stone-500 dark:text-neutral-400 italic">
        No connections required.
      </p>
    );
  }
  return (
    <table className="w-full text-xs">
      <thead>
        <tr className="text-stone-500 dark:text-neutral-400">
          <th className="text-left font-medium py-1">Provider</th>
          <th className="text-left font-medium py-1">Type</th>
          <th className="text-right font-medium py-1">Status</th>
        </tr>
      </thead>
      <tbody>
        {metas.map(m => (
          <tr
            key={m.refKey}
            className="border-t border-stone-100 dark:border-neutral-800 text-stone-700 dark:text-neutral-300">
            <td className="py-1">{m.label}</td>
            <td className="py-1">{m.mechanism}</td>
            <td className="py-1 text-right">
              <a
                href={`#${m.connectPath}`}
                className="text-primary-600 hover:underline whitespace-nowrap">
                {t('workflows.preview.connect')}
              </a>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function SettingsTable({
  timeoutSecs,
  onError,
  iterationCap,
  modelTier,
}: {
  timeoutSecs: number;
  onError: string;
  iterationCap?: number;
  modelTier: string | null;
}) {
  const rows: Array<{ k: string; v: string | number }> = [
    { k: 'timeout_secs', v: timeoutSecs },
    { k: 'on_error', v: onError },
  ];
  if (iterationCap !== undefined) {
    rows.push({ k: 'iteration_cap', v: iterationCap });
  }
  if (modelTier) {
    rows.push({ k: 'model_tier', v: modelTier });
  }
  return (
    <table className="w-full text-xs">
      <tbody>
        {rows.map(row => (
          <tr
            key={row.k}
            className="border-t border-stone-100 dark:border-neutral-800 first:border-t-0">
            <td className="py-1 font-mono text-stone-500 dark:text-neutral-400">{row.k}</td>
            <td className="py-1 text-stone-700 dark:text-neutral-300 text-right">{row.v}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
