/**
 * TypeScript types mirroring `src/openhuman/connections/types.rs`.
 *
 * Kept hand-written rather than codegen'd; the surface is small enough for
 * manual sync. Update both this file and the Rust types in lock-step.
 * See `Automations/systemsdesign.md §2.1`.
 */

export type GenericHttpConnectionId = string;

/** Encrypted secret reference; carries the `enc2:<hex>` blob. */
export interface SecretRef {
  ciphertext: string;
}

/** Discriminated union over the 6 connection mechanisms. */
export type ConnectionRef =
  | { type: 'composio'; toolkit_id: string; account_id?: string | null }
  | { type: 'channel'; provider: string; channel_id: string }
  | { type: 'webview'; provider: string; account_id: string }
  | { type: 'builtin'; integration: string }
  | { type: 'mcp'; server_id: string; tool_name?: string | null }
  | { type: 'generic_http'; connection_id: GenericHttpConnectionId };

export type ConnectionKind =
  | 'composio'
  | 'channel'
  | 'webview'
  | 'builtin'
  | 'mcp'
  | 'generic_http';

export type AuthKind =
  | { kind: 'none' }
  | { kind: 'bearer' }
  | { kind: 'basic' }
  | { kind: 'api_key_header'; name: string }
  | { kind: 'query_param'; name: string };

export type ConnectionStatus =
  | { kind: 'connected' }
  | { kind: 'not_connected' }
  | { kind: 'disabled' }
  | { kind: 'error'; reason: string };

export interface ConnectionView {
  ref: ConnectionRef;
  display_name: string;
  status: ConnectionStatus;
  last_used_at?: string | null;
  mechanism_label: string;
  /** Last probe outcome from this core session. `null` = never probed. */
  verification?: Verification | null;
}

export interface GenericHttpConnection {
  id: GenericHttpConnectionId;
  name: string;
  base_url: string;
  auth_kind: AuthKind;
  secret_ref?: SecretRef | null;
  default_headers: Array<[string, string]>;
  created_at: string;
  updated_at: string;
}

export interface NewCredential {
  secret: string;
}

export interface CreateGenericHttpRequest {
  name: string;
  base_url: string;
  auth_kind: AuthKind;
  auth_credential?: NewCredential | null;
  default_headers: Array<[string, string]>;
}

export interface UpdateGenericHttpRequest {
  name?: string | null;
  base_url?: string | null;
  auth_kind?: AuthKind | null;
  auth_credential?: NewCredential | null;
  default_headers?: Array<[string, string]> | null;
}

export interface TestProbeResult {
  ok: boolean;
  status?: number | null;
  error?: string | null;
}

export interface ConnectionsListRequest {
  kind_filter?: ConnectionKind[] | null;
  search?: string | null;
}

export interface ConnectionsListResponse {
  connections: ConnectionView[];
  generated_at: string;
}

/** Stable kind ordering used by the Hub UI. */
export const CONNECTION_KIND_ORDER: ConnectionKind[] = [
  'composio',
  'channel',
  'webview',
  'builtin',
  'mcp',
  'generic_http',
];

// ── MCP add / remove (P0-6b) ────────────────────────────────────────────

export type McpAddAuth =
  | { kind: 'none' }
  | { kind: 'bearer_token'; token: string }
  | { kind: 'basic'; username: string; password: string }
  | { kind: 'header'; name: string; value: string };

export interface McpAddRequest {
  name: string;
  endpoint?: string;
  command?: string;
  args?: string[];
  env?: Array<[string, string]>;
  cwd?: string | null;
  description?: string | null;
  auth?: McpAddAuth;
}

export interface McpServerConfig {
  name: string;
  endpoint: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  cwd?: string | null;
  description?: string | null;
  enabled: boolean;
  timeout_secs: number;
  auth: McpAddAuth;
}

// ── Verification (real probe outcomes) ─────────────────────────────────

export type VerificationResult = { kind: 'live' } | { kind: 'failed'; reason: string };

export interface Verification {
  last_probed_at: string;
  result: VerificationResult;
}
