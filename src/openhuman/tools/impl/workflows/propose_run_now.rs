//! `workflow_propose_run_now` — propose-only manual-dispatch preview (F-12).
//!
//! Returns a [`WorkflowStateProposal { action: RunNow, rationale,
//! enabled }`] payload. The `enabled` bit is `false` when the
//! workflow's health is not Ready — the UI uses it to grey out the
//! Apply button without losing the rationale ("Cannot run: missing
//! `gmail` connection").
//!
//! The `rationale` carries a duration estimate computed server-side
//! from the median of the last few successful runs, fallback
//! "unknown" when the workflow has no run history. The agent uses
//! this to set user expectations before the click.

use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::workflows::ops;
use crate::openhuman::workflows::store::Pagination;
use crate::openhuman::workflows::types::{
    RunStatus, StateAction, Workflow, WorkflowHealth, WorkflowStateProposal,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// Median is computed over the last `SAMPLE_SIZE` successful runs so
/// a recent outage doesn't skew the estimate. Picked from the same
/// drawer as ranker / heartbeat windows: small enough to react to
/// behavioural change, big enough to dampen single-run noise.
const SAMPLE_SIZE: u32 = 5;

pub struct WorkflowProposeRunNowTool {
    config: Arc<Config>,
}

impl WorkflowProposeRunNowTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WorkflowProposeRunNowTool {
    fn name(&self) -> &str {
        super::TOOL_WORKFLOW_PROPOSE_RUN_NOW
    }

    fn description(&self) -> &str {
        "PREVIEW-ONLY: propose firing a manual run of a workflow. \
         Returns a `WorkflowStateProposal { action: RunNow, rationale, \
         enabled }`. `enabled: false` means health != Ready and the UI \
         should grey out the Apply button. This tool does NOT mutate \
         (the user's Apply click triggers `workflows_run_now`)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": { "type": "string", "description": "Workflow id to propose running." }
            },
            "required": ["workflow_id"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let workflow_id = args
            .get("workflow_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("missing required field `workflow_id`"))?;
        tracing::info!(
            target: "workflows-agent",
            "[workflows-agent] tool=workflow_propose_run_now wf={workflow_id}"
        );
        let workflow = match ops::get(&self.config, workflow_id.clone())
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .value
        {
            Some(w) => w,
            None => {
                return Ok(ToolResult::success(serde_json::to_string(&json!({
                    "error": "not_found",
                    "workflow_id": workflow_id,
                }))?))
            }
        };

        if !matches!(workflow.health, WorkflowHealth::Ready) {
            // Health-gated. UI greys out Apply; rationale is the
            // human-readable reason the agent can echo.
            let preview = WorkflowStateProposal {
                workflow_id: workflow.id,
                action: StateAction::RunNow,
                rationale: health_blocked_rationale(&workflow.health),
                enabled: false,
            };
            let json_str = serde_json::to_string(&preview)?;
            let data_b64 = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                json_str.as_bytes(),
            );
            let preview_tag =
                format!("<workflow-preview kind=\"state\" data=\"{data_b64}\"></workflow-preview>");
            let payload = json!({
                "status": "state_preview_ready",
                "render_instructions": "Include the `preview_tag` value verbatim in your user-facing reply. Do not call workflow_propose_run_now again — the user clicks Apply on the rendered card.",
                "preview_tag": preview_tag,
                "state_proposal": preview,
            });
            let markdown = format!(
                "Run-now is blocked by missing connections. Include this tag verbatim in your reply, then stop — do NOT call this tool again.\n\n{preview_tag}"
            );
            return Ok(ToolResult::success_with_markdown(payload, markdown));
        }

        let estimate = estimate_duration(&self.config, &workflow).await;
        let preview = WorkflowStateProposal {
            workflow_id: workflow.id,
            action: StateAction::RunNow,
            rationale: vec![format!("Estimated time: {estimate}.")],
            enabled: true,
        };
        let json_str = serde_json::to_string(&preview)?;
        let data_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            json_str.as_bytes(),
        );
        let preview_tag =
            format!("<workflow-preview kind=\"state\" data=\"{data_b64}\"></workflow-preview>");
        let payload = json!({
            "status": "state_preview_ready",
            "render_instructions": "Include the `preview_tag` value verbatim in your user-facing reply. Do not call workflow_propose_run_now again — the user clicks Apply on the rendered card.",
            "preview_tag": preview_tag,
            "state_proposal": preview,
        });
        let markdown = format!(
            "Run-now preview ready. Include this tag verbatim in your reply, then stop — do NOT call this tool again.\n\n{preview_tag}"
        );
        Ok(ToolResult::success_with_markdown(payload, markdown))
    }
}

fn health_blocked_rationale(health: &WorkflowHealth) -> Vec<String> {
    match health {
        WorkflowHealth::Ready => vec![],
        WorkflowHealth::NeedsConnections { missing } => {
            let names: Vec<String> = missing.iter().map(|r| format!("{r:?}")).collect();
            vec![format!(
                "Cannot run: missing connections [{}].",
                names.join(", ")
            )]
        }
        WorkflowHealth::LastRunFailed { reason, .. } => {
            vec![format!("Cannot run: last run failed ({reason}).")]
        }
        WorkflowHealth::SessionExpired { connection, .. } => {
            vec![format!("Cannot run: session expired for {connection:?}.")]
        }
    }
}

/// Median wall-clock duration of the last [`SAMPLE_SIZE`] successful
/// runs, expressed as a human-readable string. Returns "unknown
/// (no past runs)" when the workflow has zero successful history —
/// the agent surfaces this so the user knows the estimate is
/// missing rather than mistakenly read as "zero seconds".
async fn estimate_duration(config: &Config, workflow: &Workflow) -> String {
    let pagination = Pagination {
        limit: SAMPLE_SIZE,
        offset: 0,
    };
    let runs = match ops::list_runs(config, workflow.id.clone(), pagination).await {
        Ok(o) => o.value,
        Err(err) => {
            tracing::warn!(
                target: "workflows-agent",
                "[workflows-agent] estimate_duration list_runs failed wf={}: {err:#}; falling back to unknown",
                workflow.id
            );
            return "unknown (history unavailable)".into();
        }
    };
    let mut durations_ms: Vec<i64> = runs
        .iter()
        .filter(|r| matches!(r.status, RunStatus::Succeeded))
        .filter_map(|r| {
            r.completed_at
                .map(|c| (c - r.started_at).num_milliseconds())
        })
        .filter(|d| *d >= 0)
        .collect();
    if durations_ms.is_empty() {
        return "unknown (no past runs)".into();
    }
    durations_ms.sort_unstable();
    let mid = durations_ms[durations_ms.len() / 2];
    format_duration_ms(mid)
}

fn format_duration_ms(ms: i64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{mins}m {secs}s")
    }
}
