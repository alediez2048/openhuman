/**
 * F4-13 — click-to-edit primitives for trivial campaign fields.
 *
 * No LLM round-trip: change → save → server commit, optimistic UI.
 * The (D)-hybrid editor split from the 2026-05-26 grill: trivial
 * fields use these widgets; reasoning edits route through chat
 * (F4-16).
 */
import { useEffect, useRef, useState } from 'react';

interface InlineTextProps {
  value: string;
  onSave: (next: string) => Promise<void> | void;
  placeholder?: string;
  maxLength?: number;
  testId?: string;
  className?: string;
  /** Render as a heading inside the static view (h1, h3, span). */
  as?: 'h1' | 'h3' | 'span';
}

/** Click-to-edit single-line text. Saves on Enter or blur; cancels on Escape. */
export function InlineText({
  value,
  onSave,
  placeholder,
  maxLength = 120,
  testId,
  className,
  as = 'span',
}: InlineTextProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const [saving, setSaving] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!editing) setDraft(value);
  }, [value, editing]);

  useEffect(() => {
    if (editing) inputRef.current?.focus();
  }, [editing]);

  const commit = async () => {
    if (saving) return;
    const trimmed = draft.trim();
    if (!trimmed) {
      // Empty isn't valid for the trivial-text path; revert.
      setDraft(value);
      setEditing(false);
      return;
    }
    if (trimmed === value) {
      setEditing(false);
      return;
    }
    setSaving(true);
    try {
      await onSave(trimmed);
      setEditing(false);
    } catch {
      // Caller surfaces the error; revert on failure.
      setDraft(value);
      setEditing(false);
    } finally {
      setSaving(false);
    }
  };

  if (editing) {
    return (
      <input
        ref={inputRef}
        type="text"
        value={draft}
        onChange={e => setDraft(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={e => {
          if (e.key === 'Enter') void commit();
          if (e.key === 'Escape') {
            setDraft(value);
            setEditing(false);
          }
        }}
        maxLength={maxLength}
        placeholder={placeholder}
        disabled={saving}
        data-testid={testId ? `${testId}-input` : undefined}
        className={`${className ?? ''} bg-white dark:bg-neutral-900 border border-primary-400 rounded px-1.5 py-0.5 focus:outline-none focus:ring-2 focus:ring-primary-500`}
      />
    );
  }

  const StaticTag = as;
  return (
    <StaticTag
      role="button"
      tabIndex={0}
      onClick={() => setEditing(true)}
      onKeyDown={e => {
        if (e.key === 'Enter') setEditing(true);
      }}
      data-testid={testId}
      title="Click to edit"
      className={`${className ?? ''} cursor-text hover:bg-stone-50 dark:hover:bg-neutral-800 rounded px-1 -mx-1`}>
      {value || placeholder || '—'}
    </StaticTag>
  );
}

interface InlineTextareaProps {
  value: string;
  onSave: (next: string) => Promise<void> | void;
  placeholder?: string;
  maxLength?: number;
  testId?: string;
  className?: string;
}

/** Click-to-edit multi-line text. Cmd+Enter saves; Escape cancels. */
export function InlineTextarea({
  value,
  onSave,
  placeholder,
  maxLength = 2000,
  testId,
  className,
}: InlineTextareaProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const [saving, setSaving] = useState(false);
  const ref = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    if (!editing) setDraft(value);
  }, [value, editing]);

  useEffect(() => {
    if (editing) ref.current?.focus();
  }, [editing]);

  const commit = async () => {
    if (saving) return;
    const trimmed = draft.trim();
    if (trimmed === value.trim()) {
      setEditing(false);
      return;
    }
    setSaving(true);
    try {
      await onSave(trimmed);
      setEditing(false);
    } catch {
      setDraft(value);
      setEditing(false);
    } finally {
      setSaving(false);
    }
  };

  if (editing) {
    return (
      <textarea
        ref={ref}
        value={draft}
        onChange={e => setDraft(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={e => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) void commit();
          if (e.key === 'Escape') {
            setDraft(value);
            setEditing(false);
          }
        }}
        maxLength={maxLength}
        placeholder={placeholder}
        disabled={saving}
        rows={3}
        data-testid={testId ? `${testId}-textarea` : undefined}
        className={`${className ?? ''} w-full bg-white dark:bg-neutral-900 border border-primary-400 rounded px-2 py-1 text-sm focus:outline-none focus:ring-2 focus:ring-primary-500`}
      />
    );
  }

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => setEditing(true)}
      onKeyDown={e => {
        if (e.key === 'Enter') setEditing(true);
      }}
      data-testid={testId}
      title="Click to edit"
      className={`${className ?? ''} cursor-text hover:bg-stone-50 dark:hover:bg-neutral-800 rounded px-1 -mx-1`}>
      {value || <span className="text-stone-400 italic">{placeholder || 'Add description…'}</span>}
    </div>
  );
}
