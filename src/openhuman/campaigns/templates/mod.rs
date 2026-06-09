//! F4-17 — bundled starter-campaign catalog.
//!
//! Three RU-10/11/12 templates demonstrating the campaign shape
//! end-to-end. Each ships as JSON via `include_str!` so a malformed
//! file surfaces at build time (the parse test catches it) rather
//! than at runtime.
//!
//! Mirrors `workflows/templates/mod.rs` for shape — `BUNDLED_JSON`
//! tuple list + `all_bundled()` parser + `raw_payload_for(id)`
//! lookup — so the catalog UI can pick up campaign templates with
//! the same patterns it already uses for workflow templates.

use serde::{Deserialize, Serialize};

use crate::openhuman::campaigns::types::{ApprovalPolicy, EntityRef, OutcomeSpec, Throttle};
use crate::openhuman::connections::types::ConnectionRef;
use crate::openhuman::workflows::types::StarterTemplate;

pub mod ops;

pub const RU_10_JSON: &str = include_str!("ru-10-vendor-outreach.json");
pub const RU_11_JSON: &str = include_str!("ru-11-content-calendar.json");
pub const RU_12_JSON: &str = include_str!("ru-12-ads-monitor.json");

pub const BUNDLED_JSON: &[(&str, &str)] = &[
    ("ru-10-vendor-outreach", RU_10_JSON),
    ("ru-11-content-calendar", RU_11_JSON),
    ("ru-12-ads-monitor", RU_12_JSON),
];

/// One row in the campaign starter catalog. Wraps a campaign
/// definition + the proposed sub-workflows the user gets when they
/// click [Use template]. The proposed workflows reuse
/// [`StarterTemplate`] from the workflows domain so the executor
/// can dispatch them without a separate type system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CampaignTemplate {
    pub template_id: String,
    pub min_phase: u32,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub entity_binding: EntityRef,
    #[serde(default)]
    pub throttle: Option<Throttle>,
    pub approval_policy: ApprovalPolicy,
    #[serde(default)]
    pub target_outcome: Option<OutcomeSpec>,
    /// Sub-workflows the apply path creates alongside the campaign.
    /// Reuses the existing workflow-template shape — the campaign-
    /// shaped fields (entity binding, throttle, approval policy)
    /// live on the campaign, not on each child.
    pub proposed_workflows: Vec<StarterTemplate>,
    #[serde(default)]
    pub required_connections: Vec<ConnectionRef>,
    #[serde(default)]
    pub rationale_at_seed: Vec<String>,
}

/// Read-only catalog view of a campaign template. Mirrors
/// [`crate::openhuman::workflows::types::StarterTemplateView`] for
/// the workflow catalog: identifying fields + a humanised summary
/// + the connection-health overlay + the full raw payload for the
/// apply-from-template path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CampaignTemplateView {
    pub template_id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// One-line summary like "Attio · 20/day · draft & approve".
    pub summary: String,
    pub required_connections: Vec<ConnectionRef>,
    pub missing_connections: Vec<ConnectionRef>,
    pub workflow_count: usize,
    #[serde(default)]
    pub rationale_at_seed: Vec<String>,
    pub raw_payload: serde_json::Value,
}

/// Parse every bundled template into a typed [`CampaignTemplate`].
/// A malformed file gets logged + skipped; the parse test catches it
/// in CI before it can ship.
pub fn all_bundled() -> Vec<CampaignTemplate> {
    BUNDLED_JSON
        .iter()
        .filter_map(
            |(label, raw)| match serde_json::from_str::<CampaignTemplate>(raw) {
                Ok(t) => Some(t),
                Err(err) => {
                    tracing::error!(
                        target: "campaigns",
                        "[campaign-templates] failed to parse `{label}`: {err}"
                    );
                    None
                }
            },
        )
        .collect()
}

/// Raw JSON for a single template by id. Used by the apply path
/// so the full body round-trips into `campaigns_apply_template`
/// without a re-serialise loop.
pub fn raw_payload_for(template_id: &str) -> Option<serde_json::Value> {
    BUNDLED_JSON
        .iter()
        .find_map(|(label, raw)| (*label == template_id).then(|| serde_json::from_str(raw).ok()))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_template_parses() {
        for (label, raw) in BUNDLED_JSON {
            let parsed: Result<CampaignTemplate, _> = serde_json::from_str(raw);
            assert!(parsed.is_ok(), "{label} failed to parse: {parsed:?}");
        }
    }

    #[test]
    fn all_bundled_returns_three_templates() {
        assert_eq!(all_bundled().len(), 3);
    }

    #[test]
    fn raw_payload_for_known_id_returns_json() {
        assert!(raw_payload_for("ru-10-vendor-outreach").is_some());
        assert!(raw_payload_for("ru-12-ads-monitor").is_some());
        assert!(raw_payload_for("does-not-exist").is_none());
    }

    #[test]
    fn every_template_has_at_least_one_workflow_and_a_binding() {
        for tpl in all_bundled() {
            assert!(
                !tpl.proposed_workflows.is_empty(),
                "{} has no workflows",
                tpl.template_id
            );
            assert!(
                !tpl.required_connections.is_empty(),
                "{} declares no required connections",
                tpl.template_id
            );
        }
    }
}
