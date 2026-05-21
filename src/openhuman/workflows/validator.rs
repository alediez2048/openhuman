//! Deterministic proposal validator (NFR-2.1.5: < 50 ms).
//!
//! [`validate`] is the safety boundary that converts "the LLM emitted
//! some JSON" into "a workflow that can be safely persisted". It runs
//! purely against the deserialised [`WorkflowProposal`] + the live
//! [`ConnectionsSnapshot`] — no LLM calls, no I/O — so failures are
//! deterministic, fast, and tractable to fuzz.
//!
//! ## Checks
//!
//! Every check maps to one [`ProposalValidationError`] variant:
//!
//! - **Required scalars** ([`MissingRequiredField`]) — `name`,
//!   `description`, `nodes` non-empty.
//! - **Allowed node kinds** ([`UnsupportedNodeKind`]) — per
//!   [`allowed_node_kinds`]; Phase 1 only permits `AgentPrompt`.
//! - **Cron parse** ([`InvalidCron`]) — routes the expression through
//!   [`crate::openhuman::cron::normalize_expression`] (5-field →
//!   6-field translation) and then the `cron` crate's parser.
//! - **Edge integrity** ([`EdgeIntegrity`]) — every `edges[].from` and
//!   `edges[].to` must reference a node id present in `nodes`.
//! - **Required connections** ([`UnknownConnection`]) — every entry of
//!   `required_connections` must be "live" in the snapshot. The
//!   returned variant carries up to 3 fuzzy [`fuzzy_candidates`] so
//!   the next retry attempt can correct typos surgically.
//! - **Per-node `allowed_connections`** ([`UnknownConnection`]) — same
//!   live-snapshot check applied to each node's
//!   [`AgentPromptConfig::allowed_connections`]; protects against the
//!   LLM listing a connection in a node that doesn't appear in the
//!   top-level `required_connections`.
//!
//! The order is fixed and shallow-to-deep: cheap structural checks
//! before the snapshot walk so the common "missing field" failure
//! returns immediately. Each call is sub-50 ms on real-world proposals
//! (NFR-2.1.5); a regression test in `validator_tests.rs` asserts the
//! ceiling.
//!
//! [`MissingRequiredField`]: ProposalValidationError::MissingRequiredField
//! [`UnsupportedNodeKind`]: ProposalValidationError::UnsupportedNodeKind
//! [`InvalidCron`]: ProposalValidationError::InvalidCron
//! [`EdgeIntegrity`]: ProposalValidationError::EdgeIntegrity
//! [`UnknownConnection`]: ProposalValidationError::UnknownConnection

use crate::openhuman::connections::types::ConnectionRef;
use crate::openhuman::cron::normalize_expression;
use crate::openhuman::workflows::health::ConnectionsSnapshot;
use crate::openhuman::workflows::types::{
    NodeConfig, NodeKind, ProposalValidationError, Trigger, WorkflowProposal,
};
use cron::Schedule as CronSchedule;
use std::collections::HashSet;
use std::str::FromStr;

/// Node kinds the proposal validator accepts at the given phase.
///
/// Phase 1 = `[AgentPrompt]` only. Phase 2 will add `ToolCall`,
/// `HttpRequest`, `ChannelMessage`, `Condition`, `Delay`, `Transform`,
/// `AwaitHumanApproval`. Phase 3 adds `FanOut`. The validator surfaces
/// `UnsupportedNodeKind { node_kind, phase }` for anything outside the
/// returned slice — see ADR-019.
pub fn allowed_node_kinds(phase: u32) -> &'static [NodeKind] {
    match phase {
        1 => &[NodeKind::AgentPrompt],
        2 => &[
            NodeKind::AgentPrompt,
            NodeKind::ToolCall,
            NodeKind::HttpRequest,
            NodeKind::ChannelMessage,
            NodeKind::Condition,
            NodeKind::Delay,
            NodeKind::Transform,
            NodeKind::AwaitHumanApproval,
        ],
        _ => &[
            NodeKind::AgentPrompt,
            NodeKind::ToolCall,
            NodeKind::HttpRequest,
            NodeKind::ChannelMessage,
            NodeKind::Condition,
            NodeKind::Delay,
            NodeKind::Transform,
            NodeKind::AwaitHumanApproval,
            NodeKind::FanOut,
        ],
    }
}

/// Validate a proposal against the user's current connections.
///
/// Returns `Ok(())` only when every check passes. The order is
/// structural-first so the common "name is empty" / "no nodes"
/// failure returns immediately without walking the snapshot.
///
/// Sub-50 ms per NFR-2.1.5 — pure Rust, no I/O, allocates a small
/// `HashSet<&str>` for the edge-integrity check and a `HashSet`
/// during fuzzy-candidate computation. A timing regression test
/// in `validator_tests.rs` enforces the ceiling on the RU-1
/// fixture.
pub fn validate(
    proposal: &WorkflowProposal,
    connections: &ConnectionsSnapshot,
    phase: u32,
) -> Result<(), ProposalValidationError> {
    tracing::debug!(
        target: "workflows-validator",
        "[workflows-validator] validate phase={phase} nodes={n} edges={e} required={r}",
        n = proposal.nodes.len(),
        e = proposal.edges.len(),
        r = proposal.required_connections.len(),
    );

    // ── Required scalars ───────────────────────────────────────────────
    if proposal.name.trim().is_empty() {
        return Err(ProposalValidationError::MissingRequiredField {
            field: "name".into(),
        });
    }
    if proposal.description.trim().is_empty() {
        return Err(ProposalValidationError::MissingRequiredField {
            field: "description".into(),
        });
    }
    if proposal.nodes.is_empty() {
        return Err(ProposalValidationError::MissingRequiredField {
            field: "nodes".into(),
        });
    }

    // ── Cron trigger parse ─────────────────────────────────────────────
    if let Trigger::Cron { expr, .. } = &proposal.trigger {
        validate_cron_expr(expr)?;
    }

    // ── Allowed node kinds ─────────────────────────────────────────────
    let allowed = allowed_node_kinds(phase);
    for node in &proposal.nodes {
        if !allowed.contains(&node.kind) {
            return Err(ProposalValidationError::UnsupportedNodeKind {
                node_kind: node.kind,
                phase,
            });
        }
    }

    // ── Edge integrity ─────────────────────────────────────────────────
    let node_ids: HashSet<&str> = proposal.nodes.iter().map(|n| n.id.as_str()).collect();
    for edge in &proposal.edges {
        if !node_ids.contains(edge.from.as_str()) {
            return Err(ProposalValidationError::EdgeIntegrity {
                from: edge.from.clone(),
                to: edge.to.clone(),
                reason: format!("edge `from` references unknown node id `{}`", edge.from),
            });
        }
        if !node_ids.contains(edge.to.as_str()) {
            return Err(ProposalValidationError::EdgeIntegrity {
                from: edge.from.clone(),
                to: edge.to.clone(),
                reason: format!("edge `to` references unknown node id `{}`", edge.to),
            });
        }
    }

    // ── required_connections ⊆ snapshot ────────────────────────────────
    for r in &proposal.required_connections {
        if !connections.is_connected(r) {
            return Err(ProposalValidationError::UnknownConnection {
                r#ref: r.clone(),
                candidates: fuzzy_candidates(r, connections),
            });
        }
    }

    // ── per-node allowed_connections ⊆ snapshot ────────────────────────
    for node in &proposal.nodes {
        let NodeConfig::AgentPrompt(cfg) = &node.config;
        for r in &cfg.allowed_connections {
            if !connections.is_connected(r) {
                return Err(ProposalValidationError::UnknownConnection {
                    r#ref: r.clone(),
                    candidates: fuzzy_candidates(r, connections),
                });
            }
        }
    }

    Ok(())
}

/// Lift cron-expression validation behind a single helper so the
/// proposer + the run-time validator share the same parse rules.
/// The error surface is the structured [`InvalidCron`] variant; the
/// `parse_error` body is `cron::Error::Display` so the drafting
/// agent's retry prompt can echo it back to the LLM.
///
/// [`InvalidCron`]: ProposalValidationError::InvalidCron
fn validate_cron_expr(expr: &str) -> Result<(), ProposalValidationError> {
    let normalised =
        normalize_expression(expr).map_err(|err| ProposalValidationError::InvalidCron {
            expr: expr.to_string(),
            parse_error: format!("{err:#}"),
        })?;
    CronSchedule::from_str(&normalised).map_err(|err| ProposalValidationError::InvalidCron {
        expr: expr.to_string(),
        parse_error: err.to_string(),
    })?;
    Ok(())
}

/// Up to 3 fuzzy matches for `unknown` against the user's actual
/// connections. Surfaced via
/// [`ProposalValidationError::UnknownConnection::candidates`] so the
/// drafting agent can correct typos on its next attempt without
/// guessing — "you said `gmaill`; did you mean `gmail`?".
///
/// The metric is a damerau-style char-shift count via
/// [`levenshtein`]; same-mechanism connections are preferred over
/// cross-mechanism ones (a Composio typo suggests other Composio
/// rows, not a Channel row that happens to share a substring).
pub fn fuzzy_candidates(
    unknown: &ConnectionRef,
    snapshot: &ConnectionsSnapshot,
) -> Vec<ConnectionRef> {
    let needle = name_for_fuzzy(unknown);
    if needle.is_empty() {
        return Vec::new();
    }
    let unknown_kind = std::mem::discriminant(unknown);
    let mut scored: Vec<(usize, ConnectionRef)> = snapshot
        .views()
        .iter()
        .filter(|v| std::mem::discriminant(&v.r#ref) == unknown_kind)
        .filter(|v| &v.r#ref != unknown)
        .filter_map(|v| {
            let candidate_name = name_for_fuzzy(&v.r#ref);
            if candidate_name.is_empty() {
                return None;
            }
            let d = levenshtein(needle, &candidate_name);
            // Only keep candidates within 3 edits — beyond that the
            // "suggestion" is noise.
            if d <= 3 {
                Some((d, v.r#ref.clone()))
            } else {
                None
            }
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0));
    scored.into_iter().take(3).map(|(_d, r)| r).collect()
}

/// Identifier the fuzzy matcher compares against. We deliberately
/// match on the mechanism's "name" field rather than the entire
/// `ConnectionRef` JSON: the drafting agent's typos almost always
/// land in the toolkit/provider name, not in account ids.
fn name_for_fuzzy(r: &ConnectionRef) -> &str {
    match r {
        ConnectionRef::Composio { toolkit_id, .. } => toolkit_id.as_str(),
        ConnectionRef::Channel { provider, .. } => provider.as_str(),
        ConnectionRef::Webview { provider, .. } => provider.as_str(),
        ConnectionRef::Builtin { integration } => integration.as_str(),
        ConnectionRef::Mcp { server_id, .. } => server_id.as_str(),
        ConnectionRef::GenericHttp { connection_id } => connection_id.as_str(),
    }
}

/// Classic O(n·m) Levenshtein distance over chars (not bytes — so
/// multi-byte connection names compare correctly). Bounded by 3 in
/// the caller; for n,m ≤ ~32 (mechanism name length) this is
/// trivially under 50 µs.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (curr[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}
