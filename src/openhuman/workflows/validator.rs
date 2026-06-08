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
    BackoffSpec, NodeConfig, NodeKind, ProposalValidationError, Trigger, WorkflowProposal,
};
use cron::Schedule as CronSchedule;
use std::collections::HashSet;
use std::str::FromStr;

/// Node kinds the proposal validator accepts at the given phase.
///
/// Phase 1 = `[AgentPrompt]` only. Phase 2 (F2-1) lands `ToolCall`,
/// `HttpRequest`, `ChannelMessage`, `Condition`, `Delay`. `Transform`
/// and `AwaitHumanApproval` remain unreachable in Phase 2 because
/// their `NodeConfig::*` payload shapes haven't been designed yet
/// (deserialisation would fail before the validator even sees the
/// node — leaving them out of the allowed list keeps the validator
/// error honest). Phase 3 adds `FanOut`. The validator surfaces
/// `UnsupportedNodeKind { node_kind, phase }` for anything outside
/// the returned slice — see ADR-019.
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
        ],
        3 => &[
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
        // Phase 4+ adds `ForEach` (F4-7). Transform / AwaitHumanApproval /
        // FanOut still ride along — they were Phase-3 placeholders.
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
            NodeKind::ForEach,
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

    // ── Cron trigger parse + active_hours (F2-15) ──────────────────────
    if let Trigger::Cron {
        expr, active_hours, ..
    } = &proposal.trigger
    {
        validate_cron_expr(expr)?;
        if let Some(hours) = active_hours {
            validate_active_hours(&hours.start, &hours.end)?;
        }
    }

    // ── ChannelMessage trigger validation (F2-11) ──────────────────────
    //
    // Provider is required; the filter (when present) is structurally
    // checked: regex compiles via the `regex` crate, `from_user` is
    // non-empty when set. Matching the channel/event provider against
    // the live channels snapshot is intentionally OUT — the validator
    // stays I/O-free; an unknown provider becomes a fail-soft no-fire
    // (no events ever match) rather than a synchronous reject.
    if let Trigger::ChannelMessage { provider, filter } = &proposal.trigger {
        if provider.trim().is_empty() {
            return Err(ProposalValidationError::MissingRequiredField {
                field: "trigger.provider".into(),
            });
        }
        if let Some(f) = filter {
            if let Some(ref user) = f.from_user {
                if user.trim().is_empty() {
                    return Err(ProposalValidationError::MissingRequiredField {
                        field: "trigger.filter.from_user".into(),
                    });
                }
            }
            if let Some(ref pattern) = f.regex {
                if let Err(err) = regex::Regex::new(pattern) {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        // No NodeId — borrow the trigger label so the
                        // error stays distinct from per-node failures.
                        node_id: "trigger".into(),
                        node_kind: NodeKind::ChannelMessage,
                        reason: format!("trigger.filter.regex does not compile: {err}"),
                    });
                }
            }
        }
    }

    // ── Composio trigger non-empty (F2-10) ─────────────────────────────
    //
    // Validating against the live Composio trigger catalog would require
    // I/O (network or cache lookup), which conflicts with the validator's
    // pure-Rust + sub-50 ms contract (NFR-2.1.5). Per the F2-10 brainstorm
    // lean: enforce non-empty `toolkit` / `trigger_id` here, and surface
    // a deeper "unknown trigger" error from the subscriber's dispatch
    // path (where I/O is already allowed). A LIKE-pre-filter miss in
    // `list_workflows_matching_composio_event` is the natural fail-soft
    // path — the workflow is saved but simply never fires.
    if let Trigger::ComposioEvent {
        toolkit,
        trigger_id,
    } = &proposal.trigger
    {
        if toolkit.trim().is_empty() {
            return Err(ProposalValidationError::MissingRequiredField {
                field: "trigger.toolkit".into(),
            });
        }
        if trigger_id.trim().is_empty() {
            return Err(ProposalValidationError::MissingRequiredField {
                field: "trigger.trigger_id".into(),
            });
        }
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

    // ── per-NodeConfig shape validation ────────────────────────────────
    //
    // F2-1 adds the Phase 2 variants. Each one gets a shallow shape
    // check here (non-empty strings, bounded delay, etc.); per-kind
    // dispatch-time checks (tool-registry lookup, connection-id
    // resolution against the encrypted-credential store) live in the
    // per-kind executor bodies (F2-3..F2-7) so the validator stays
    // I/O-free per NFR-2.1.5.
    //
    // F2-8 also validates the per-node `retry_policy` bounds: OQ-21
    // locks the surface (`requirements.md §8`).
    for node in &proposal.nodes {
        if let Some(retry) = &node.retry_policy {
            if retry.max_attempts < 1 || retry.max_attempts > 5 {
                return Err(ProposalValidationError::InvalidNodeConfig {
                    node_id: node.id.clone(),
                    node_kind: node.kind,
                    reason: format!(
                        "retry_policy.max_attempts must be in [1, 5]; got {}",
                        retry.max_attempts
                    ),
                });
            }
            match &retry.backoff {
                BackoffSpec::Exponential { initial_ms, max_ms } => {
                    if *initial_ms < 100 || *initial_ms > 10_000 {
                        return Err(ProposalValidationError::InvalidNodeConfig {
                            node_id: node.id.clone(),
                            node_kind: node.kind,
                            reason: format!(
                                "retry_policy.backoff.initial_ms must be in [100, 10000]; got {initial_ms}"
                            ),
                        });
                    }
                    if *max_ms > 60_000 {
                        return Err(ProposalValidationError::InvalidNodeConfig {
                            node_id: node.id.clone(),
                            node_kind: node.kind,
                            reason: format!(
                                "retry_policy.backoff.max_ms must be ≤ 60000; got {max_ms}"
                            ),
                        });
                    }
                    if *max_ms < *initial_ms {
                        return Err(ProposalValidationError::InvalidNodeConfig {
                            node_id: node.id.clone(),
                            node_kind: node.kind,
                            reason: format!(
                                "retry_policy.backoff.max_ms ({max_ms}) must be ≥ initial_ms ({initial_ms})"
                            ),
                        });
                    }
                }
            }
        }
    }
    for node in &proposal.nodes {
        match &node.config {
            NodeConfig::AgentPrompt(cfg) => {
                for r in &cfg.allowed_connections {
                    if !connections.is_connected(r) {
                        return Err(ProposalValidationError::UnknownConnection {
                            r#ref: r.clone(),
                            candidates: fuzzy_candidates(r, connections),
                        });
                    }
                }
            }
            NodeConfig::ToolCall(cfg) => {
                if cfg.tool_name.trim().is_empty() {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: "tool_call.tool_name must be non-empty".into(),
                    });
                }
            }
            NodeConfig::HttpRequest(cfg) => {
                if cfg.connection_id.trim().is_empty() {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: "http_request.connection_id must be non-empty".into(),
                    });
                }
                if cfg.path_template.trim().is_empty() {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: "http_request.path_template must be non-empty".into(),
                    });
                }
            }
            NodeConfig::ChannelMessage(cfg) => {
                if cfg.connection_id.trim().is_empty() {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: "channel_message.connection_id must be non-empty".into(),
                    });
                }
                if cfg.body_template.trim().is_empty() {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: "channel_message.body_template must be non-empty".into(),
                    });
                }
            }
            NodeConfig::Condition(cfg) => {
                // `left` may be a templated reference (whole `{{...}}`
                // tokens are pre-substituted at dispatch); we only
                // reject EMPTY because that's the un-fillable form.
                if cfg.left.is_empty() {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: "condition.left must be non-empty".into(),
                    });
                }
                // F2-6: NodeId references must resolve. Catches
                // typos in the drafter's output AND a misaligned
                // copy-paste from a different workflow.
                let known_ids: std::collections::HashSet<&str> =
                    proposal.nodes.iter().map(|n| n.id.as_str()).collect();
                if !known_ids.contains(cfg.then_node_id.as_str()) {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: format!(
                            "condition.then_node_id `{}` references a node that doesn't exist",
                            cfg.then_node_id
                        ),
                    });
                }
                if let Some(else_id) = &cfg.else_node_id {
                    if !known_ids.contains(else_id.as_str()) {
                        return Err(ProposalValidationError::InvalidNodeConfig {
                            node_id: node.id.clone(),
                            node_kind: node.kind,
                            reason: format!(
                                "condition.else_node_id `{else_id}` references a node that doesn't exist"
                            ),
                        });
                    }
                }
            }
            NodeConfig::Delay(cfg) => {
                if cfg.seconds == 0 {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: "delay.seconds must be > 0".into(),
                    });
                }
                // 24-hour cap: a runaway workflow shouldn't sleep
                // forever. Refine in F2-7 if a concrete use case
                // needs longer.
                if cfg.seconds > 86_400 {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: "delay.seconds must be ≤ 86400 (24h)".into(),
                    });
                }
            }
            NodeConfig::ForEach(cfg) => {
                if cfg.max_per_run == 0 || cfg.max_per_run > 1000 {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: format!(
                            "for_each.max_per_run must be in [1, 1000] (got {})",
                            cfg.max_per_run
                        ),
                    });
                }
                if let Some(secs) = cfg.per_iteration_delay_secs {
                    if secs > 3600 {
                        return Err(ProposalValidationError::InvalidNodeConfig {
                            node_id: node.id.clone(),
                            node_kind: node.kind,
                            reason: format!(
                                "for_each.per_iteration_delay_secs must be ≤ 3600 (got {secs})"
                            ),
                        });
                    }
                }
                if cfg.body_nodes.is_empty() {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: "for_each.body_nodes must contain at least one node id".into(),
                    });
                }
                let known_ids: std::collections::HashSet<&str> =
                    proposal.nodes.iter().map(|n| n.id.as_str()).collect();
                for body_id in &cfg.body_nodes {
                    if !known_ids.contains(body_id.as_str()) {
                        return Err(ProposalValidationError::InvalidNodeConfig {
                            node_id: node.id.clone(),
                            node_kind: node.kind,
                            reason: format!(
                                "for_each.body_nodes id `{body_id}` references a node that doesn't exist"
                            ),
                        });
                    }
                    // A for_each body must not point at itself (would
                    // infinitely recurse the dispatcher).
                    if body_id == &node.id {
                        return Err(ProposalValidationError::InvalidNodeConfig {
                            node_id: node.id.clone(),
                            node_kind: node.kind,
                            reason: "for_each.body_nodes must not include the for_each node itself"
                                .into(),
                        });
                    }
                }
                // entity_binding presence: must be Some when the
                // proposal isn't part of a campaign. We don't know
                // here whether the runtime workflow belongs to a
                // campaign (proposals have no campaign_id), so we
                // only require non-`None` when the implicit campaign
                // hand-off is unavailable — for now, require it
                // explicit and let the executor surface the
                // inheritance path when running. F4-10 will allow
                // `None` once the drafter knows whether it's inside
                // a campaign.
                if cfg.entity_binding.is_none() {
                    return Err(ProposalValidationError::InvalidNodeConfig {
                        node_id: node.id.clone(),
                        node_kind: node.kind,
                        reason: "for_each.entity_binding must be set on standalone workflows \
                                 (campaign-inheritance lands with F4-10)"
                            .into(),
                    });
                }
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

/// F2-15: enforce `start` / `end` shape + ordering. Both must be
/// `HH:MM` 24-hour strings (zero-padded), and `start` must be strictly
/// less than `end`. Wraparound windows like `"22:00" - "02:00"` aren't
/// supported — the user splits them into two workflows.
fn validate_active_hours(start: &str, end: &str) -> Result<(), ProposalValidationError> {
    let start_minutes =
        parse_hhmm(start).ok_or_else(|| ProposalValidationError::InvalidNodeConfig {
            node_id: "trigger".into(),
            node_kind: crate::openhuman::workflows::types::NodeKind::AgentPrompt,
            reason: format!("trigger.active_hours.start must be `HH:MM` 24-hour (got `{start}`)"),
        })?;
    let end_minutes =
        parse_hhmm(end).ok_or_else(|| ProposalValidationError::InvalidNodeConfig {
            node_id: "trigger".into(),
            node_kind: crate::openhuman::workflows::types::NodeKind::AgentPrompt,
            reason: format!("trigger.active_hours.end must be `HH:MM` 24-hour (got `{end}`)"),
        })?;
    if start_minutes >= end_minutes {
        return Err(ProposalValidationError::InvalidNodeConfig {
            node_id: "trigger".into(),
            node_kind: crate::openhuman::workflows::types::NodeKind::AgentPrompt,
            reason: format!(
                "trigger.active_hours requires start ({start}) < end ({end}) — wraparound windows aren't supported; split into two workflows"
            ),
        });
    }
    Ok(())
}

/// Parse `HH:MM` (24-hour, zero-padded) → minutes-since-midnight.
/// Returns `None` for any malformed input so the caller can render a
/// clear validator error.
fn parse_hhmm(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    if parts[0].len() != 2 || parts[1].len() != 2 {
        return None;
    }
    let h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
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
