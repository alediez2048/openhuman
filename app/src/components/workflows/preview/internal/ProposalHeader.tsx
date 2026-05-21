/**
 * Header row for `<WorkflowProposalPreview>` + `<WorkflowEditPreview>`:
 * an icon + name + confidence dot + optional description line.
 *
 * Keeps the visual language consistent across the proposal /
 * edit / delete / state preview cards (per ADR-020's "one visual
 * language").
 */
import { useT } from '../../../../lib/i18n/I18nContext';
import type { Confidence } from '../../../../types/workflows';

const CONFIDENCE_COLOR: Record<Confidence, string> = {
  high: 'bg-sage-500',
  medium: 'bg-amber-500',
  low: 'bg-coral-500',
};

interface Props {
  icon: string;
  name: string;
  confidence?: Confidence;
  description?: string | null;
}

export function ProposalHeader({ icon, name, confidence, description }: Props) {
  const { t } = useT();
  const confidenceLabel = confidence ? t(`workflows.preview.confidence.${confidence}`) : undefined;
  return (
    <div className="flex items-start gap-3">
      <div aria-hidden className="text-xl leading-none text-primary-600 mt-0.5 select-none">
        {icon}
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 flex-wrap">
          <h3
            className="text-sm font-semibold text-stone-900 dark:text-neutral-100 truncate"
            title={name}>
            {name}
          </h3>
          {confidence && (
            <span
              aria-label={t('workflows.preview.confidence_aria').replace(
                '{label}',
                confidenceLabel ?? confidence
              )}
              className="inline-flex items-center gap-1 text-[11px] font-medium text-stone-500 dark:text-neutral-400">
              <span
                aria-hidden
                className={`w-1.5 h-1.5 rounded-full ${CONFIDENCE_COLOR[confidence]}`}
              />
              {confidenceLabel}
            </span>
          )}
        </div>
        {description && (
          <p className="text-xs text-stone-500 dark:text-neutral-400 mt-0.5">{description}</p>
        )}
      </div>
    </div>
  );
}
