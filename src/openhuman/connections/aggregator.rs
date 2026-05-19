//! Aggregates per-mechanism connection state into a unified `Vec<ConnectionView>`.
//!
//! Phase 0 / P0-1 leaves this module intentionally empty. P0-2 fills it: it
//! queries through composio, channels, webview accounts, integrations, and MCP
//! to produce a unified read-model surfaced by the `connections_list` RPC.
