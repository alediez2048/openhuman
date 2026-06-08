//! F4-3b agent-tool tests:
//! - Each tool reports its stable name.
//! - The orchestrator's allowlist carries every campaign tool name.
//! - Propose-state tools emit a `<campaign-preview>` tag on legal
//!   transitions + a structured `invalid_transition` payload on
//!   illegal ones.

use super::*;
use crate::openhuman::campaigns::store::insert_campaign;
use crate::openhuman::campaigns::types::{
    ApprovalPolicy, Campaign, CampaignStatus, EntityRef,
};
use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{Tool, ToolContent, ToolResult};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn temp_config() -> (TempDir, Arc<Config>) {
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, Arc::new(config))
}

fn seed_campaign(config: &Config, id: &str, status: CampaignStatus) {
    let now = Utc::now();
    let c = Campaign {
        id: id.into(),
        schema_version: 1,
        name: format!("Campaign {id}"),
        description: None,
        status,
        entity_binding: EntityRef::GoogleSheet {
            spreadsheet_id: "ss".into(),
            range: "A1:Z".into(),
        },
        throttle: None,
        approval_policy: ApprovalPolicy::DraftAndApprove,
        target_outcome: None,
        created_at: now,
        updated_at: now,
        last_run_at: None,
    };
    insert_campaign(config, &c).unwrap();
}

/// Concatenate everything the tool put in its result: the markdown
/// rendering (used by propose-state tools via
/// `ToolResult::success_with_markdown` — these emit a `Json` content
/// block plus the preview tag in markdown), each `Text` content
/// block, and each `Json` content block re-serialised. The tests
/// assert on substring presence, so concatenation is safe.
fn result_text(result: &ToolResult) -> String {
    let mut out = String::new();
    if let Some(md) = result.markdown_formatted.as_deref() {
        out.push_str(md);
        out.push('\n');
    }
    for c in &result.content {
        match c {
            ToolContent::Text { text } => {
                out.push_str(text);
                out.push('\n');
            }
            ToolContent::Json { data } => {
                if let Ok(s) = serde_json::to_string(data) {
                    out.push_str(&s);
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn every_tool_reports_its_stable_name() {
    let (_dir, config) = temp_config();
    assert_eq!(CampaignListTool::new(config.clone()).name(), TOOL_CAMPAIGN_LIST);
    assert_eq!(CampaignGetTool::new(config.clone()).name(), TOOL_CAMPAIGN_GET);
    assert_eq!(
        CampaignProposePauseTool::new(config.clone()).name(),
        TOOL_CAMPAIGN_PROPOSE_PAUSE
    );
    assert_eq!(
        CampaignProposeResumeTool::new(config.clone()).name(),
        TOOL_CAMPAIGN_PROPOSE_RESUME
    );
    assert_eq!(
        CampaignProposeArchiveTool::new(config).name(),
        TOOL_CAMPAIGN_PROPOSE_ARCHIVE
    );
}

#[test]
fn orchestrator_allowlist_carries_every_campaign_tool_name() {
    // Per memory `reference_orchestrator_allowlist`: every new agent
    // tool needs both global registration AND an entry in
    // `agent.toml`'s `[tools].named`. The allowlist is an explicit
    // whitelist, not a fallback.
    let toml = include_str!("../../../agent/agents/orchestrator/agent.toml");
    for name in ALL_TOOL_NAMES {
        assert!(
            toml.contains(&format!("\"{name}\"")),
            "orchestrator agent.toml MUST include `{name}` in [tools].named — \
             the orchestrator can't see the tool otherwise"
        );
    }
}

// ── Read-only ──────────────────────────────────────────────────────

#[tokio::test]
async fn campaign_list_returns_empty_on_fresh_workspace() {
    let (_dir, config) = temp_config();
    let tool = CampaignListTool::new(config);
    let result = tool.execute(json!({})).await.unwrap();
    assert!(!result.is_error);
    let body = result_text(&result);
    assert!(body.contains("\"campaigns\":[]"));
}

#[tokio::test]
async fn campaign_list_returns_seeded_rows() {
    let (_dir, config) = temp_config();
    seed_campaign(&config, "a", CampaignStatus::Active);
    seed_campaign(&config, "b", CampaignStatus::Paused);
    let tool = CampaignListTool::new(config);
    let result = tool.execute(json!({})).await.unwrap();
    let body = result_text(&result);
    assert!(body.contains("\"id\":\"a\""));
    assert!(body.contains("\"id\":\"b\""));
}

#[tokio::test]
async fn campaign_list_status_filter_narrows_results() {
    let (_dir, config) = temp_config();
    seed_campaign(&config, "active", CampaignStatus::Active);
    seed_campaign(&config, "paused", CampaignStatus::Paused);
    let tool = CampaignListTool::new(config);
    let result = tool
        .execute(json!({ "filter": { "status": "active" } }))
        .await
        .unwrap();
    let body = result_text(&result);
    assert!(body.contains("\"id\":\"active\""));
    assert!(
        !body.contains("\"id\":\"paused\""),
        "status filter must exclude Paused rows; body: {body}"
    );
}

#[tokio::test]
async fn campaign_get_returns_row_when_id_known() {
    let (_dir, config) = temp_config();
    seed_campaign(&config, "x", CampaignStatus::Active);
    let tool = CampaignGetTool::new(config);
    let result = tool.execute(json!({ "id": "x" })).await.unwrap();
    let body = result_text(&result);
    assert!(body.contains("\"id\":\"x\""));
}

#[tokio::test]
async fn campaign_get_returns_null_for_unknown_id() {
    let (_dir, config) = temp_config();
    let tool = CampaignGetTool::new(config);
    let result = tool.execute(json!({ "id": "ghost" })).await.unwrap();
    let body = result_text(&result);
    assert!(body.contains("\"campaign\":null"));
}

// ── Propose-state ───────────────────────────────────────────────────

#[tokio::test]
async fn propose_pause_on_active_emits_campaign_preview_tag() {
    let (_dir, config) = temp_config();
    seed_campaign(&config, "active", CampaignStatus::Active);
    let tool = CampaignProposePauseTool::new(config);
    let result = tool.execute(json!({ "id": "active" })).await.unwrap();
    let body = result_text(&result);
    assert!(
        body.contains("<campaign-preview"),
        "expected preview tag, got: {body}"
    );
    assert!(body.contains("action=\"pause\""));
}

#[tokio::test]
async fn propose_pause_on_draft_returns_invalid_transition_with_hint() {
    let (_dir, config) = temp_config();
    seed_campaign(&config, "draft", CampaignStatus::Draft);
    let tool = CampaignProposePauseTool::new(config);
    let result = tool.execute(json!({ "id": "draft" })).await.unwrap();
    let body = result_text(&result);
    assert!(body.contains("invalid_transition"));
    assert!(body.contains("\"hint\""));
    // No preview tag on the illegal path — error payload only.
    assert!(!body.contains("<campaign-preview"));
}

#[tokio::test]
async fn propose_resume_on_paused_emits_preview_tag() {
    let (_dir, config) = temp_config();
    seed_campaign(&config, "p", CampaignStatus::Paused);
    let tool = CampaignProposeResumeTool::new(config);
    let result = tool.execute(json!({ "id": "p" })).await.unwrap();
    let body = result_text(&result);
    assert!(body.contains("<campaign-preview"));
    assert!(body.contains("action=\"resume\""));
}

#[tokio::test]
async fn propose_archive_on_active_returns_invalid_transition() {
    let (_dir, config) = temp_config();
    seed_campaign(&config, "a", CampaignStatus::Active);
    let tool = CampaignProposeArchiveTool::new(config);
    let result = tool.execute(json!({ "id": "a" })).await.unwrap();
    let body = result_text(&result);
    assert!(body.contains("invalid_transition"));
    assert!(body.contains("wind_down"));
}

#[tokio::test]
async fn propose_archive_on_wound_down_emits_preview_tag() {
    let (_dir, config) = temp_config();
    seed_campaign(&config, "wd", CampaignStatus::WoundDown);
    let tool = CampaignProposeArchiveTool::new(config);
    let result = tool.execute(json!({ "id": "wd" })).await.unwrap();
    let body = result_text(&result);
    assert!(body.contains("<campaign-preview"));
    assert!(body.contains("action=\"archive\""));
}

#[tokio::test]
async fn propose_pause_on_unknown_id_returns_not_found_payload() {
    let (_dir, config) = temp_config();
    let tool = CampaignProposePauseTool::new(config);
    let result = tool.execute(json!({ "id": "ghost" })).await.unwrap();
    let body = result_text(&result);
    assert!(body.contains("not_found"));
}
