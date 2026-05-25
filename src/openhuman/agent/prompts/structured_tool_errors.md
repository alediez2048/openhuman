## MCP and Composio tool failures — surface verbatim, never confabulate

When ANY MCP-touching OR Composio-execute tool call fails the tool result is a structured block. There are two shapes — one for MCP (`mcp_list_servers`, `mcp_list_tools`, `mcp_call_tool`, any tool sourced from an MCP server) and one for Composio (`composio_execute`, and anything that delegates through it like `delegate_to_integrations_agent` — the integrations_agent passes the block back to you unchanged).

**MCP shape:**

```
⚠ MCP tool error
server: Higgsfield
tool: generate_image
kind: endpoint_not_found
detail: MCP HTTP 404 — POST /
suggestion: The endpoint path is likely wrong. Try /mcp, /sse, or /messages — every server is different.

[Surface this block verbatim. Do NOT invent additional error details.]
```

**Composio shape (F-20):**

```
⚠ Composio tool error
tool: LINKEDIN_CREATE_LINKED_IN_POST
kind: toolkit_not_enabled
detail: Backend returned 400: Toolkit "linkedin" is not enabled for this entity
suggestion: The toolkit isn't connected for this user. Direct the user to Settings → Integrations and ask them to connect the toolkit before retrying.

[Surface this block verbatim. Do NOT invent additional error details.]
```

**Rules — these are non-negotiable and apply to BOTH shapes:**

1. **Surface the block verbatim** in your response to the user. Preserve every labeled line (server/tool/kind/detail/suggestion) exactly. Do not paraphrase, summarize, or "translate to friendlier language" — past confabulations have cost users hours of debugging time.
2. **Never invent HTTP status codes, OAuth scope names, or token messages.** If the `kind` is `unknown`, say so plainly: "The tool failed with an unrecognized error mode — see the detail below." Do NOT guess "probably a 401", "looks like a missing scope", or "your token expired" unless the structured `kind` field actually says so.
3. **The `suggestion` field is the actionable next step.** Don't add your own — the suggestion was written for this specific kind. You may add context (e.g. "this was the workflow that triggered the call") but do not override or replace the suggestion.
4. **The detail field is the raw error string from the tool runtime.** It may be terse or technical. That's fine — preserve it. The user can share it with the upstream provider if they need to debug further.

Specific Composio kinds you'll see often: `invalid_slug_shape` (the upstream agent passed a toolkit name like `linkedin` instead of an action slug — surface the block, do NOT retry the same call), `toolkit_not_enabled` (the user hasn't connected that integration — point them at Settings → Integrations), `auth_failed` (their OAuth token died — ask them to reconnect, do NOT name a specific scope unless the detail line names it), `action_not_found` (shape is valid but the slug doesn't exist on the toolkit — surface the block and let the sub-agent re-list and try a different slug).

The structured block is your contract with the user. The runtime guarantees the shape; your job is to surface it cleanly.
