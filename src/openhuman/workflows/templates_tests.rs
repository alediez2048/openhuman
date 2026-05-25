//! F-5 — per-template parse + structural tests for the four bundled
//! RU-* JSON files.
//!
//! These run at build time so any malformed template is caught before
//! it ships. The crons are validated through the same `cron` crate
//! parser F-11's validator uses.

use super::templates::{all_bundled, BUNDLED_JSON};
use crate::openhuman::workflows::types::{StarterTemplate, Trigger};
use std::collections::HashSet;
use std::str::FromStr;

#[test]
fn every_bundled_template_parses_cleanly() {
    // `all_bundled` only logs + skips bad files in production; here we
    // verify every individual file parses so a regression surfaces in
    // CI rather than silently dropping a template.
    for (label, raw) in BUNDLED_JSON {
        let parsed: Result<StarterTemplate, _> = serde_json::from_str(raw);
        assert!(
            parsed.is_ok(),
            "template `{label}` failed to parse: {err:#?}",
            err = parsed.err()
        );
    }
    assert_eq!(all_bundled().len(), BUNDLED_JSON.len());
}

#[test]
fn every_template_has_a_parseable_cron_expression() {
    // Templates use standard 5-field crontab syntax. The `cron` crate
    // itself requires a 6/7-field expression (Quartz-style with
    // seconds), so we route through `cron::schedule::normalize_expression`
    // — the same normalizer the production scheduler uses — before
    // handing the expression to `cron::Schedule::from_str`. F-11's
    // validator will pin this same path.
    use crate::openhuman::cron::normalize_expression;
    for t in all_bundled() {
        let trigger: Trigger = serde_json::from_value(t.trigger.clone()).unwrap_or_else(|err| {
            panic!(
                "template `{}` trigger failed to deserialize: {err}",
                t.template_id
            )
        });
        if let Trigger::Cron { ref expr, .. } = trigger {
            let normalized = normalize_expression(expr).unwrap_or_else(|err| {
                panic!(
                    "template `{}` cron `{expr}` could not be normalized: {err}",
                    t.template_id
                )
            });
            cron::Schedule::from_str(&normalized).unwrap_or_else(|err| {
                panic!(
                    "template `{}` cron `{expr}` (normalized to `{normalized}`) rejected by cron::Schedule::from_str: {err}",
                    t.template_id
                )
            });
        }
    }
}

#[test]
fn every_template_has_non_empty_required_connections() {
    for t in all_bundled() {
        assert!(
            !t.required_connections.is_empty(),
            "template `{}` must declare at least one required connection",
            t.template_id
        );
    }
}

#[test]
fn template_ids_are_unique() {
    let mut seen = HashSet::new();
    for t in all_bundled() {
        assert!(
            seen.insert(t.template_id.clone()),
            "duplicate template_id `{}` — every bundled template must declare a unique id",
            t.template_id
        );
    }
}

#[test]
fn ru_1_template_id_matches_the_e2e_spec() {
    // F-15's catalog E2E spec keys on this exact string. Pin it so a
    // typo on rename never breaks the contract.
    let ids: Vec<_> = all_bundled().into_iter().map(|t| t.template_id).collect();
    assert!(
        ids.contains(&"ru-1-founder-morning-digest".to_string()),
        "RU-1 must keep its locked template_id; saw {ids:?}"
    );
}

#[test]
fn every_template_declares_min_phase_one_or_higher() {
    for t in all_bundled() {
        assert!(
            t.min_phase >= 1,
            "template `{}` declares min_phase={}, must be ≥ 1",
            t.template_id,
            t.min_phase
        );
    }
}

// ── F2-12: Phase 2 templates parse + validate at phase=2 ───────────────

/// F2-12: every bundled template must satisfy F-11's `validate` at
/// `phase = template.min_phase` so a malformed Phase 2 template is
/// caught at build time before it ships to the catalog.
///
/// Templates carry opaque JSON `nodes` / `edges` / `settings` so the
/// catalog [Add] flow can pass forward-compat fields through. This
/// test mirrors what `workflows_create` does when seeding: typed-
/// deserialize the JSON into a `WorkflowProposal`, build a synthetic
/// "live" snapshot covering every `ConnectionRef` the template refers
/// to (template `required_connections` + per-AgentPrompt
/// `allowed_connections`), then run `validate`. The synthetic snapshot
/// keeps the test focused on *structural* validity — the seeding flow
/// re-runs the validator against the user's real snapshot.
#[test]
fn every_bundled_template_validates_at_its_declared_min_phase() {
    use crate::openhuman::connections::types::{ConnectionRef, ConnectionStatus, ConnectionView};
    use crate::openhuman::connections::verification::{Verification, VerificationResult};
    use crate::openhuman::workflows::health::ConnectionsSnapshot;
    use crate::openhuman::workflows::types::{
        Confidence, Edge, Node, NodeConfig, WorkflowProposal, WorkflowSettings,
    };
    use crate::openhuman::workflows::validator::validate;
    use chrono::{TimeZone, Utc};

    fn live_view(r#ref: ConnectionRef) -> ConnectionView {
        let requires_verification = matches!(
            r#ref,
            ConnectionRef::GenericHttp { .. }
                | ConnectionRef::Channel { .. }
                | ConnectionRef::Mcp { .. }
        );
        ConnectionView {
            r#ref,
            display_name: "test".into(),
            status: ConnectionStatus::Connected,
            last_used_at: None,
            mechanism_label: "test".into(),
            verification: if requires_verification {
                Some(Verification {
                    last_probed_at: Utc.with_ymd_and_hms(2026, 5, 24, 0, 0, 0).unwrap(),
                    result: VerificationResult::Live,
                })
            } else {
                None
            },
        }
    }

    for t in all_bundled() {
        let trigger = serde_json::from_value(t.trigger.clone()).unwrap_or_else(|err| {
            panic!(
                "template `{}` trigger failed to typed-deserialize: {err}",
                t.template_id
            )
        });
        let nodes: Vec<Node> = serde_json::from_value(t.nodes.clone()).unwrap_or_else(|err| {
            panic!(
                "template `{}` nodes failed to typed-deserialize: {err}",
                t.template_id
            )
        });
        let edges: Vec<Edge> = serde_json::from_value(t.edges.clone()).unwrap_or_else(|err| {
            panic!(
                "template `{}` edges failed to typed-deserialize: {err}",
                t.template_id
            )
        });
        let settings: WorkflowSettings =
            serde_json::from_value(t.settings.clone()).unwrap_or_else(|err| {
                panic!(
                    "template `{}` settings failed to typed-deserialize: {err}",
                    t.template_id
                )
            });

        // Build the synthetic snapshot — union of template
        // `required_connections` + every AgentPrompt's
        // `allowed_connections`.
        let mut refs: Vec<ConnectionRef> = t.required_connections.clone();
        for node in &nodes {
            if let NodeConfig::AgentPrompt(cfg) = &node.config {
                for r in &cfg.allowed_connections {
                    refs.push(r.clone());
                }
            }
        }
        let views: Vec<ConnectionView> = refs.into_iter().map(live_view).collect();
        let snapshot = ConnectionsSnapshot::new(views);

        let proposal = WorkflowProposal {
            name: t.name.clone(),
            description: t.description.clone(),
            trigger,
            nodes,
            edges,
            settings,
            required_connections: t.required_connections.clone(),
            rationale: t.rationale_at_seed.clone(),
            confidence: Confidence::High,
        };

        validate(&proposal, &snapshot, t.min_phase).unwrap_or_else(|err| {
            panic!(
                "template `{}` failed validator at phase={}: {err:?}",
                t.template_id, t.min_phase
            )
        });
    }
}

/// F2-12 pins the Phase 2 catalog: RU-5..RU-9 must all be present
/// with `min_phase >= 2`. Regression guard against accidental removal
/// or downgrade.
#[test]
fn phase_2_templates_are_present_and_declare_min_phase_2() {
    let phase_2_ids = [
        "ru-5-stripe-payment-thank-you",
        "ru-6-slack-mention-triage",
        "ru-7-github-issue-summary",
        "ru-8-daily-sales-rollup",
        "ru-9-zapier-bridge",
    ];
    let bundled: HashSet<String> = all_bundled().into_iter().map(|t| t.template_id).collect();
    for id in &phase_2_ids {
        assert!(
            bundled.contains(*id),
            "Phase 2 template `{id}` missing from bundle"
        );
    }
    for t in all_bundled()
        .into_iter()
        .filter(|t| phase_2_ids.contains(&t.template_id.as_str()))
    {
        assert!(
            t.min_phase >= 2,
            "Phase 2 template `{}` declares min_phase={}, must be ≥ 2",
            t.template_id,
            t.min_phase
        );
    }
}

/// F2-12: Phase 2 triggers cover the four new shapes (composio_event,
/// channel_message, webhook x2, cron) — confirm at least one bundled
/// template exercises each.
#[test]
fn phase_2_templates_cover_each_new_trigger_kind() {
    let mut saw_composio = false;
    let mut saw_channel_message = false;
    let mut saw_webhook = false;
    for t in all_bundled() {
        if t.min_phase < 2 {
            continue;
        }
        let trigger: Trigger = serde_json::from_value(t.trigger.clone()).unwrap_or_else(|err| {
            panic!(
                "template `{}` trigger typed-deserialize failed: {err}",
                t.template_id
            )
        });
        match trigger {
            Trigger::ComposioEvent { .. } => saw_composio = true,
            Trigger::ChannelMessage { .. } => saw_channel_message = true,
            Trigger::Webhook { .. } => saw_webhook = true,
            _ => {}
        }
    }
    assert!(
        saw_composio,
        "no Phase 2 template uses Trigger::ComposioEvent"
    );
    assert!(
        saw_channel_message,
        "no Phase 2 template uses Trigger::ChannelMessage"
    );
    assert!(saw_webhook, "no Phase 2 template uses Trigger::Webhook");
}
