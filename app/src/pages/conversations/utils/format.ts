export function formatRelativeTime(dateStr: string): string {
  const now = Date.now();
  const then = new Date(dateStr).getTime();
  const diffMs = now - then;
  if (diffMs < 60_000) return 'just now';
  const mins = Math.floor(diffMs / 60_000);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function getInlineCompletionSuffix(input: string, suggestion: string): string {
  if (!input || !suggestion) return '';
  const normalize = (value: string) =>
    value
      .replace(/\u2192/g, ' ')
      .replace(/\s+/g, ' ')
      .trim();

  const normalizedInput = normalize(input);
  const normalizedSuggestion = normalize(suggestion);
  if (!normalizedSuggestion) return '';

  if (normalizedSuggestion.startsWith(normalizedInput)) {
    return normalizedSuggestion.slice(normalizedInput.length).trimStart();
  }

  const maxOverlap = Math.min(normalizedInput.length, normalizedSuggestion.length, 120);
  for (let overlap = maxOverlap; overlap >= 1; overlap -= 1) {
    if (
      normalizedInput.slice(normalizedInput.length - overlap) ===
      normalizedSuggestion.slice(0, overlap)
    ) {
      return normalizedSuggestion.slice(overlap).trimStart();
    }
  }

  if (normalizedInput.endsWith(normalizedSuggestion)) {
    return '';
  }
  return normalizedSuggestion;
}

export function buildAcceptedInlineCompletion(input: string, suffix: string): string {
  const normalizedInput = input.replace(/\u2192/g, ' ').replace(/\t+/g, ' ');
  const cleanSuffix = suffix
    .replace(/\u2192/g, ' ')
    .replace(/\t+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();

  if (!cleanSuffix) return normalizedInput;

  const needsSpace =
    normalizedInput.length > 0 && !/\s$/.test(normalizedInput) && !/^[,.;:!?)]/.test(cleanSuffix);

  return `${normalizedInput}${needsSpace ? ' ' : ''}${cleanSuffix}`;
}

export function isAllowedExternalHref(rawHref: string): boolean {
  try {
    const url = new URL(rawHref);
    return url.protocol === 'http:' || url.protocol === 'https:' || url.protocol === 'mailto:';
  } catch {
    return false;
  }
}

/**
 * Custom inline tag the welcome agent (and any future agent) can drop
 * inside a chat bubble to render an in-app navigation pill, e.g.
 *
 *     <openhuman-link path="settings/notifications">Allow notifications</openhuman-link>
 *
 * The conversation UI (`AgentMessageBubble`) parses these out of the
 * raw text, splitting the message into ordered text/link segments.
 * Text segments still render through Markdown; link segments render as
 * a clickable pill that calls `react-router`'s navigate(`/${path}`) on
 * click — no deep-link round-trip, no host browser involvement.
 *
 * Path is the hash route under HashRouter (e.g. `settings/notifications`
 * → `#/settings/notifications`). Leading/trailing slashes are tolerated.
 */
export interface OpenhumanLinkSegment {
  kind: 'link';
  path: string;
  label: string;
}

/**
 * Inline tag the workflow drafting sub-agent (F-11/F-12 propose
 * tools) embeds in its response to render a `<WorkflowProposalPreview>`
 * (or `<WorkflowEditPreview>` / `<WorkflowDeletePreview>` /
 * `<WorkflowStatePreview>`) inside the chat bubble:
 *
 *     <workflow-preview kind="proposal" data='{...json...}'></workflow-preview>
 *
 * `kind` discriminates the renderer ('proposal' | 'edit' | 'delete'
 * | 'state'); `data` is the JSON-serialised payload the
 * matching component expects. Single quotes around `data` keep the
 * inner JSON's double quotes from clashing with the attribute.
 */
export interface WorkflowPreviewSegment {
  kind: 'workflow_preview';
  previewKind: 'proposal' | 'edit' | 'delete' | 'state';
  data: string;
}

export interface TextSegment {
  kind: 'text';
  text: string;
}

export type BubbleSegment = TextSegment | OpenhumanLinkSegment | WorkflowPreviewSegment;

const OPENHUMAN_LINK_RE =
  /<openhuman-link\s+path=(?:"([^"]+)"|'([^']+)')\s*>([\s\S]*?)<\/openhuman-link>/gi;

// Match `<workflow-preview kind="proposal" data='{...}'></workflow-preview>`.
// `data` accepts either single or double quotes for the outer wrapper;
// the agent prompt instructs the LLM to use single quotes so the JSON's
// double quotes nest cleanly.
const WORKFLOW_PREVIEW_RE =
  /<workflow-preview\s+kind=(?:"([^"]+)"|'([^']+)')\s+data=(?:"([\s\S]*?)"|'([\s\S]*?)')\s*>\s*<\/workflow-preview>/gi;

/**
 * Combined tag-extraction pass: walks the content and splits it
 * into text + tagged segments preserving order.
 *
 * Both the `<openhuman-link>` and `<workflow-preview>` tags are
 * handled in a single sweep so a message that mixes them (rare
 * but possible — e.g. a workflow preview plus a "Configure
 * notifications" link) renders both segments in their original
 * positions.
 */
export function parseBubbleSegments(content: string): BubbleSegment[] {
  if (
    !content ||
    (!content.includes('<openhuman-link') && !content.includes('<workflow-preview'))
  ) {
    return [{ kind: 'text', text: content }];
  }
  // Collect every match from both regexes with its index range, then
  // emit segments in order.
  type Hit = { start: number; end: number; segment: BubbleSegment };
  const hits: Hit[] = [];

  OPENHUMAN_LINK_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = OPENHUMAN_LINK_RE.exec(content)) !== null) {
    const path = (m[1] ?? m[2] ?? '').trim().replace(/^\/+/, '').replace(/\/+$/, '');
    const label = (m[3] ?? '').trim();
    if (path && label) {
      hits.push({
        start: m.index,
        end: m.index + m[0].length,
        segment: { kind: 'link', path, label },
      });
    }
  }

  WORKFLOW_PREVIEW_RE.lastIndex = 0;
  while ((m = WORKFLOW_PREVIEW_RE.exec(content)) !== null) {
    const kindStr = (m[1] ?? m[2] ?? '').trim();
    const dataStr = (m[3] ?? m[4] ?? '').trim();
    if (
      (kindStr === 'proposal' ||
        kindStr === 'edit' ||
        kindStr === 'delete' ||
        kindStr === 'state') &&
      dataStr
    ) {
      hits.push({
        start: m.index,
        end: m.index + m[0].length,
        segment: { kind: 'workflow_preview', previewKind: kindStr, data: dataStr },
      });
    }
  }

  hits.sort((a, b) => a.start - b.start);

  const segments: BubbleSegment[] = [];
  let cursor = 0;
  for (const hit of hits) {
    if (hit.start < cursor) continue; // overlap — drop the later
    if (hit.start > cursor) {
      segments.push({ kind: 'text', text: content.slice(cursor, hit.start) });
    }
    segments.push(hit.segment);
    cursor = hit.end;
  }
  if (cursor < content.length) {
    segments.push({ kind: 'text', text: content.slice(cursor) });
  }
  return segments;
}

export type AgentBubblePosition = 'single' | 'first' | 'middle' | 'last';

export function getAgentBubbleChrome(position: AgentBubblePosition): string {
  if (position === 'single') return 'rounded-2xl rounded-bl-md';
  if (position === 'first') return 'rounded-2xl rounded-bl-lg';
  if (position === 'middle') return 'rounded-2xl rounded-tl-md rounded-bl-lg';
  return 'rounded-2xl rounded-tl-md rounded-bl-md';
}

export function formatResetTime(isoStr: string): string {
  const ms = new Date(isoStr).getTime() - Date.now();
  if (ms <= 0) return 'now';
  const mins = Math.ceil(ms / 60_000);
  if (mins < 60) return `in ${mins}m`;
  const hours = Math.floor(mins / 60);
  const remMins = mins % 60;
  if (hours < 24) return remMins > 0 ? `in ${hours}h ${remMins}m` : `in ${hours}h`;
  const days = Math.floor(hours / 24);
  const remHours = hours % 24;
  return remHours > 0 ? `in ${days}d ${remHours}h` : `in ${days}d`;
}
