//! Human-friendly workflow diff (F-12 `workflow_propose_update`).
//!
//! [`workflow_diff`] compares two [`Workflow`] values field-by-field
//! and produces a flat `Vec<String>` of bullets the
//! `<WorkflowProposalPreview>` component (F-14) renders verbatim.
//! Server-side computation keeps the diff stable across UI versions
//! and matches the agent's rationale on the same payload.
//!
//! ## Diff coverage
//!
//! - `name` — rename bullet.
//! - `description` — change bullet (clipped to 80 chars per side
//!   for readability; full content stays in the JSON payload).
//! - `trigger` — discriminator change (Manual ↔ Cron, etc.) and
//!   for cron-to-cron edits the `expr` / `tz` deltas.
//! - `settings.timeout_secs`, `settings.on_error`.
//! - `nodes` — length, per-position `prompt` line-count delta,
//!   `allowed_connections` set adds + removes.
//!
//! ## Bullet cap
//!
//! The output is capped at [`MAX_DIFF_BULLETS`] (20). When more
//! changes are detected, the final bullet is
//! `"… and N more changes."` so the preview doesn't blow the
//! chat-render budget. Phase 1 proposals are single-node so the cap
//! mostly matters for compound rename + reschedule cases; Phase 2
//! multi-node graphs lean on the cap heavily.

use crate::openhuman::connections::types::ConnectionRef;
use crate::openhuman::workflows::types::{NodeConfig, Trigger, Workflow};

/// Bullet cap per FR-1.13.7 / NFR-2.5.6. Diffs longer than this
/// truncate with a `… and N more changes.` tail bullet.
pub const MAX_DIFF_BULLETS: usize = 20;

/// Compare two workflows and return a flat bullet list of changes.
///
/// Returns `[]` when `current == proposed` so the preview surface
/// can short-circuit on "no-op edits" without an extra equality
/// check in the caller.
pub fn workflow_diff(current: &Workflow, proposed: &Workflow) -> Vec<String> {
    let mut bullets: Vec<String> = Vec::new();

    if current.name != proposed.name {
        bullets.push(format!(
            "Renamed from \"{}\" to \"{}\".",
            current.name, proposed.name
        ));
    }

    if current.description != proposed.description {
        bullets.push(match (&current.description, &proposed.description) {
            (None, Some(p)) => format!("Added description: \"{}\".", clip(p)),
            (Some(_), None) => "Cleared description.".to_string(),
            (Some(c), Some(p)) => format!(
                "Changed description from \"{}\" to \"{}\".",
                clip(c),
                clip(p)
            ),
            (None, None) => unreachable!("checked != above"),
        });
    }

    bullets.extend(diff_trigger(&current.trigger, &proposed.trigger));

    if current.settings.timeout_secs != proposed.settings.timeout_secs {
        bullets.push(format!(
            "Changed timeout from {}s to {}s.",
            current.settings.timeout_secs, proposed.settings.timeout_secs
        ));
    }
    if current.settings.on_error != proposed.settings.on_error {
        bullets.push(format!(
            "Changed on_error policy from {:?} to {:?}.",
            current.settings.on_error, proposed.settings.on_error
        ));
    }

    bullets.extend(diff_nodes(&current.nodes, &proposed.nodes));

    cap(bullets)
}

fn clip(s: &str) -> String {
    const LIMIT: usize = 80;
    if s.chars().count() <= LIMIT {
        return s.to_string();
    }
    let snippet: String = s.chars().take(LIMIT).collect();
    format!("{snippet}…")
}

fn diff_trigger(current: &Trigger, proposed: &Trigger) -> Vec<String> {
    if current == proposed {
        return Vec::new();
    }
    match (current, proposed) {
        (
            Trigger::Cron {
                expr: c_e,
                tz: c_tz,
                active_hours: c_ah,
            },
            Trigger::Cron {
                expr: p_e,
                tz: p_tz,
                active_hours: p_ah,
            },
        ) => {
            let mut out = Vec::new();
            if c_e != p_e {
                out.push(format!("Changed cron schedule from `{c_e}` to `{p_e}`."));
            }
            if c_tz != p_tz {
                out.push(format!(
                    "Changed cron timezone from {:?} to {:?}.",
                    c_tz, p_tz
                ));
            }
            if c_ah != p_ah {
                out.push("Changed active hours.".into());
            }
            out
        }
        (a, b) => vec![format!(
            "Changed trigger from {} to {}.",
            trigger_label(a),
            trigger_label(b)
        )],
    }
}

fn trigger_label(t: &Trigger) -> &'static str {
    match t {
        Trigger::Cron { .. } => "Cron",
        Trigger::Manual => "Manual",
        Trigger::Webhook { .. } => "Webhook",
        Trigger::ComposioEvent { .. } => "Composio event",
        Trigger::ChannelMessage { .. } => "Channel message",
    }
}

fn diff_nodes(
    current: &[crate::openhuman::workflows::types::Node],
    proposed: &[crate::openhuman::workflows::types::Node],
) -> Vec<String> {
    let mut out = Vec::new();
    if current.len() != proposed.len() {
        out.push(format!(
            "Changed node count from {} to {}.",
            current.len(),
            proposed.len()
        ));
    }
    let pairs = current.len().min(proposed.len());
    for i in 0..pairs {
        let step = i + 1;
        let c = &current[i];
        let p = &proposed[i];
        if c.kind != p.kind {
            out.push(format!(
                "Changed step {step} kind from {:?} to {:?}.",
                c.kind, p.kind
            ));
        }
        let NodeConfig::AgentPrompt(c_cfg) = &c.config;
        let NodeConfig::AgentPrompt(p_cfg) = &p.config;

        if c_cfg.prompt != p_cfg.prompt {
            let c_lines = c_cfg.prompt.lines().count();
            let p_lines = p_cfg.prompt.lines().count();
            out.push(format!(
                "Rewrote step {step} prompt ({c_lines} → {p_lines} lines)."
            ));
        }
        if c_cfg.iteration_cap != p_cfg.iteration_cap {
            out.push(format!(
                "Changed step {step} iteration cap from {} to {}.",
                c_cfg.iteration_cap, p_cfg.iteration_cap
            ));
        }
        if c_cfg.model_tier != p_cfg.model_tier {
            out.push(format!(
                "Changed step {step} model tier from {:?} to {:?}.",
                c_cfg.model_tier, p_cfg.model_tier
            ));
        }
        out.extend(diff_connections(
            step,
            &c_cfg.allowed_connections,
            &p_cfg.allowed_connections,
        ));
    }
    out
}

fn diff_connections(
    step: usize,
    current: &[ConnectionRef],
    proposed: &[ConnectionRef],
) -> Vec<String> {
    let mut out = Vec::new();
    for r in proposed {
        if !current.contains(r) {
            out.push(format!(
                "Added {} to step {step}'s connections.",
                connection_label(r)
            ));
        }
    }
    for r in current {
        if !proposed.contains(r) {
            out.push(format!(
                "Removed {} from step {step}'s connections.",
                connection_label(r)
            ));
        }
    }
    out
}

fn connection_label(r: &ConnectionRef) -> String {
    match r {
        ConnectionRef::Composio { toolkit_id, .. } => format!("`{toolkit_id}` (Composio)"),
        ConnectionRef::Channel { provider, .. } => format!("`{provider}` (Channel)"),
        ConnectionRef::Webview { provider, .. } => format!("`{provider}` (Browser)"),
        ConnectionRef::Builtin { integration } => format!("`{integration}` (Built-in)"),
        ConnectionRef::Mcp { server_id, .. } => format!("`{server_id}` (MCP)"),
        ConnectionRef::GenericHttp { connection_id } => format!("`{connection_id}` (HTTP)"),
    }
}

fn cap(mut bullets: Vec<String>) -> Vec<String> {
    if bullets.len() <= MAX_DIFF_BULLETS {
        return bullets;
    }
    let extra = bullets.len() - (MAX_DIFF_BULLETS - 1);
    bullets.truncate(MAX_DIFF_BULLETS - 1);
    bullets.push(format!("… and {extra} more changes."));
    bullets
}
