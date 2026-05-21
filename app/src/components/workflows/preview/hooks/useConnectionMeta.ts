/**
 * Static-registry lookup for `ConnectionRef` display metadata.
 * Returns one `ConnectionMeta` per ref: the chip label the user
 * recognises (`gmail`, `slack`, …), the human mechanism label
 * (`Composio`, `Channel`, …), and the deep link into `/connections`
 * filtered to the relevant section.
 *
 * No network call: every value is derivable from the
 * `ConnectionRef` itself. The hook shape keeps the preview-card
 * call sites stable if a future ticket extends this to async
 * (e.g. pulling rich logos from the aggregator).
 */
import { useMemo } from 'react';

import type { ConnectionRef } from '../../../../types/connections';

export interface ConnectionMeta {
  /** Stable chip label, e.g. `"gmail"`. */
  label: string;
  /** Mechanism label, e.g. `"Composio"` / `"Channel"`. */
  mechanism: string;
  /** Deep link the chip's [Connect →] hyperlinks to. */
  connectPath: string;
  /** Identifier used as the React key when mapping a list of refs. */
  refKey: string;
}

function labelFor(r: ConnectionRef): string {
  switch (r.type) {
    case 'composio':
      return r.toolkit_id;
    case 'channel':
      return r.provider;
    case 'webview':
      return r.provider;
    case 'builtin':
      return r.integration;
    case 'mcp':
      return r.server_id;
    case 'generic_http':
      return r.connection_id;
  }
}

function mechanismFor(r: ConnectionRef): string {
  switch (r.type) {
    case 'composio':
      return 'Composio';
    case 'channel':
      return 'Channel';
    case 'webview':
      return 'Browser';
    case 'builtin':
      return 'Built-in';
    case 'mcp':
      return 'MCP';
    case 'generic_http':
      return 'HTTP';
  }
}

function connectPathFor(r: ConnectionRef): string {
  // `/connections` exposes section anchors by mechanism so the deep
  // link lands the user on the relevant block without a search.
  switch (r.type) {
    case 'composio':
      return '/connections#composio';
    case 'channel':
      return '/connections#channels';
    case 'webview':
      return '/connections#browser';
    case 'builtin':
      return '/connections#builtin';
    case 'mcp':
      return '/connections#mcp';
    case 'generic_http':
      return '/connections#http';
  }
}

function refKeyFor(r: ConnectionRef): string {
  // Stable per-ref key for React `key` props. Includes the
  // mechanism prefix so a Composio `gmail` doesn't collide with a
  // Channel `gmail`.
  switch (r.type) {
    case 'composio':
      return `composio:${r.toolkit_id}:${r.account_id ?? ''}`;
    case 'channel':
      return `channel:${r.provider}:${r.channel_id}`;
    case 'webview':
      return `webview:${r.provider}:${r.account_id}`;
    case 'builtin':
      return `builtin:${r.integration}`;
    case 'mcp':
      return `mcp:${r.server_id}:${r.tool_name ?? ''}`;
    case 'generic_http':
      return `http:${r.connection_id}`;
  }
}

export function metaForRef(r: ConnectionRef): ConnectionMeta {
  return {
    label: labelFor(r),
    mechanism: mechanismFor(r),
    connectPath: connectPathFor(r),
    refKey: refKeyFor(r),
  };
}

export function useConnectionMeta(refs: ConnectionRef[]): ConnectionMeta[] {
  return useMemo(() => refs.map(metaForRef), [refs]);
}
