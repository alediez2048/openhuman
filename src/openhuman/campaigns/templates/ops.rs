//! F4-17 — apply a starter campaign template.
//!
//! `apply_template` is the [Use template] click path: stamp a fresh
//! Campaign row in Draft, create one Workflow row per
//! `proposed_workflows[]` entry with `campaign_id` set to bind it,
//! and return the campaign id so the UI can navigate to the detail
//! view. Workflows ship `enabled: false` so the user reviews +
//! customises before anything fires.

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use super::{all_bundled, raw_payload_for, CampaignTemplate, CampaignTemplateView};
use crate::openhuman::campaigns::store as campaign_store;
use crate::openhuman::campaigns::types::{Campaign, CampaignStatus, ThrottleWindow};
use crate::openhuman::config::Config;
use crate::openhuman::workflows::health::ConnectionsSnapshot;
use crate::openhuman::workflows::store as wf_store;
use crate::openhuman::workflows::types::{
    OnErrorPolicy, Trigger, Workflow, WorkflowHealth, WorkflowOrigin, WorkflowSettings,
};

/// Catalog rows for the `campaigns_list_starter_templates` RPC.
/// Each row carries a humanised summary + the per-row missing-
/// connections overlay so the UI card can render the same
/// "ready / needs X" indicator the workflow catalog has.
pub fn list_starter_templates(connections: &ConnectionsSnapshot) -> Vec<CampaignTemplateView> {
    all_bundled()
        .into_iter()
        .map(|tpl| {
            let missing: Vec<_> = tpl
                .required_connections
                .iter()
                .filter(|r| !connections.is_connected(r))
                .cloned()
                .collect();
            let raw_payload = raw_payload_for(&tpl.template_id).unwrap_or(serde_json::Value::Null);
            CampaignTemplateView {
                template_id: tpl.template_id.clone(),
                name: tpl.name.clone(),
                description: tpl.description.clone(),
                tags: tpl.tags.clone(),
                summary: summarise(&tpl),
                required_connections: tpl.required_connections.clone(),
                missing_connections: missing,
                workflow_count: tpl.proposed_workflows.len(),
                rationale_at_seed: tpl.rationale_at_seed.clone(),
                raw_payload,
            }
        })
        .collect()
}

/// Apply a template — creates a Draft Campaign + one disabled
/// Workflow per `proposed_workflows[]` entry with `campaign_id`
/// linking them. Returns the new campaign id.
pub fn apply_template(config: &Config, template_id: &str) -> Result<String> {
    let tpl = all_bundled()
        .into_iter()
        .find(|t| t.template_id == template_id)
        .ok_or_else(|| anyhow::anyhow!("unknown campaign template: {template_id}"))?;

    let now = Utc::now();
    let campaign = Campaign {
        id: Uuid::new_v4().to_string(),
        schema_version: 1,
        name: tpl.name.clone(),
        description: Some(tpl.description.clone()),
        status: CampaignStatus::Draft,
        entity_binding: tpl.entity_binding.clone(),
        throttle: tpl.throttle.clone(),
        approval_policy: tpl.approval_policy.clone(),
        target_outcome: tpl.target_outcome.clone(),
        created_at: now,
        updated_at: now,
        last_run_at: None,
    };
    campaign_store::insert_campaign(config, &campaign)
        .context("apply_template: insert campaign")?;

    for wf_tpl in &tpl.proposed_workflows {
        let trigger: Trigger = serde_json::from_value(wf_tpl.trigger.clone())
            .with_context(|| format!("apply_template: parse trigger for {}", wf_tpl.template_id))?;
        let nodes = serde_json::from_value(wf_tpl.nodes.clone())
            .with_context(|| format!("apply_template: parse nodes for {}", wf_tpl.template_id))?;
        let edges = if wf_tpl.edges.is_null() {
            Vec::new()
        } else {
            serde_json::from_value(wf_tpl.edges.clone())
                .with_context(|| format!("apply_template: parse edges for {}", wf_tpl.template_id))?
        };
        let settings: WorkflowSettings = if wf_tpl.settings.is_null() {
            WorkflowSettings::default()
        } else {
            serde_json::from_value(wf_tpl.settings.clone()).unwrap_or_else(|_| WorkflowSettings {
                timeout_secs: 600,
                on_error: OnErrorPolicy::Halt,
            })
        };
        let wf = Workflow {
            id: Uuid::new_v4().to_string(),
            schema_version: 1,
            name: wf_tpl.name.clone(),
            description: Some(wf_tpl.description.clone()),
            enabled: false,
            origin: WorkflowOrigin::Imported,
            health: WorkflowHealth::Ready, // re-computed downstream
            trigger,
            nodes,
            edges,
            settings,
            created_at: now,
            updated_at: now,
            last_run_at: None,
            campaign_id: Some(campaign.id.clone()),
        };
        wf_store::insert_workflow(config, &wf)
            .with_context(|| format!("apply_template: insert workflow {}", wf_tpl.template_id))?;
    }
    Ok(campaign.id)
}

fn summarise(tpl: &CampaignTemplate) -> String {
    let mut parts = Vec::with_capacity(3);
    parts.push(match &tpl.entity_binding {
        crate::openhuman::campaigns::types::EntityRef::GoogleSheet { .. } => "Google Sheets".to_string(),
        crate::openhuman::campaigns::types::EntityRef::Attio { object_type, .. } => {
            format!("Attio · {object_type}")
        }
    });
    if let Some(t) = &tpl.throttle {
        let suffix = match t.window {
            ThrottleWindow::PerDay => "/day",
            ThrottleWindow::PerHour => "/hour",
            ThrottleWindow::PerMinute => "/min",
        };
        parts.push(format!("{}{suffix}", t.max_per_window));
    }
    use crate::openhuman::campaigns::types::ApprovalPolicy;
    parts.push(
        match &tpl.approval_policy {
            ApprovalPolicy::DraftAndApprove => "draft & approve",
            ApprovalPolicy::AutoReply => "auto-reply",
            ApprovalPolicy::Triage => "triage",
            ApprovalPolicy::Tiered { .. } => "tiered",
        }
        .to_string(),
    );
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh() -> (TempDir, Config) {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        (dir, config)
    }

    #[test]
    fn list_starter_templates_returns_three_rows_with_missing_overlay() {
        let (_dir, config) = fresh();
        let snapshot = ConnectionsSnapshot::default();
        let views = list_starter_templates(&snapshot);
        assert_eq!(views.len(), 3);
        for v in &views {
            assert!(!v.summary.is_empty());
            // No connections in snapshot → every required connection is missing.
            assert_eq!(v.missing_connections.len(), v.required_connections.len());
            let _ = &config; // keep the unused warning quiet
        }
    }

    #[test]
    fn apply_template_creates_campaign_and_linked_workflows() {
        let (_dir, config) = fresh();
        let campaign_id = apply_template(&config, "ru-10-vendor-outreach").unwrap();
        let campaign = campaign_store::get_campaign(&config, &campaign_id)
            .unwrap()
            .expect("campaign row present");
        assert!(matches!(campaign.status, CampaignStatus::Draft));
        // Two sub-workflows in the RU-10 template.
        let linked = campaign_store::list_workflow_ids_for_campaign(&config, &campaign_id).unwrap();
        assert_eq!(linked.len(), 2, "ru-10 ships 2 sub-workflows");
    }

    #[test]
    fn apply_template_rejects_unknown_id() {
        let (_dir, config) = fresh();
        let err = apply_template(&config, "does-not-exist").unwrap_err();
        assert!(err.to_string().contains("unknown campaign template"));
    }
}
