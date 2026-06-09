//! Controller schemas + registry for the Campaigns domain (F4-3).
//!
//! Mirrors the workflows-domain registration pattern. Exports
//! `all_campaigns_controller_schemas` / `all_campaigns_registered_controllers`
//! that `src/core/all.rs` composes into the global registry.

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::campaigns::store::ListCampaignsFilter;
use crate::openhuman::campaigns::types::{
    CampaignStatus, CreateCampaignRequest, UpdateCampaignRequest,
};
use crate::openhuman::config::rpc as config_rpc;
use crate::rpc::RpcOutcome;
use serde::Serialize;
use serde_json::{Map, Value};

/// Every schema this domain declares.
pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list"),
        schemas("get"),
        schemas("create"),
        schemas("update"),
        schemas("delete"),
        schemas("restore"),
        schemas("pause"),
        schemas("resume"),
        schemas("wind_down"),
        schemas("archive"),
        schemas("add_workflow"),
        schemas("remove_workflow"),
        schemas("throttle_status"),
        schemas("approvals_list_pending"),
        schemas("approvals_get"),
        schemas("approvals_approve"),
        schemas("approvals_reject"),
        schemas("approvals_batch_approve"),
    ]
}

/// Every controller (schema + handler) this domain registers.
pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list"),
            handler: handle_list,
        },
        RegisteredController {
            schema: schemas("get"),
            handler: handle_get,
        },
        RegisteredController {
            schema: schemas("create"),
            handler: handle_create,
        },
        RegisteredController {
            schema: schemas("update"),
            handler: handle_update,
        },
        RegisteredController {
            schema: schemas("delete"),
            handler: handle_delete,
        },
        RegisteredController {
            schema: schemas("restore"),
            handler: handle_restore,
        },
        RegisteredController {
            schema: schemas("pause"),
            handler: handle_pause,
        },
        RegisteredController {
            schema: schemas("resume"),
            handler: handle_resume,
        },
        RegisteredController {
            schema: schemas("wind_down"),
            handler: handle_wind_down,
        },
        RegisteredController {
            schema: schemas("archive"),
            handler: handle_archive,
        },
        RegisteredController {
            schema: schemas("add_workflow"),
            handler: handle_add_workflow,
        },
        RegisteredController {
            schema: schemas("remove_workflow"),
            handler: handle_remove_workflow,
        },
        RegisteredController {
            schema: schemas("throttle_status"),
            handler: handle_throttle_status,
        },
        RegisteredController {
            schema: schemas("approvals_list_pending"),
            handler: handle_approvals_list_pending,
        },
        RegisteredController {
            schema: schemas("approvals_get"),
            handler: handle_approvals_get,
        },
        RegisteredController {
            schema: schemas("approvals_approve"),
            handler: handle_approvals_approve,
        },
        RegisteredController {
            schema: schemas("approvals_reject"),
            handler: handle_approvals_reject,
        },
        RegisteredController {
            schema: schemas("approvals_batch_approve"),
            handler: handle_approvals_batch_approve,
        },
    ]
}

/// Aliases used by `core/all.rs` to compose every domain's surface.
pub fn all_campaigns_controller_schemas() -> Vec<ControllerSchema> {
    all_controller_schemas()
}

pub fn all_campaigns_registered_controllers() -> Vec<RegisteredController> {
    all_registered_controllers()
}

// ── Schema declarations ────────────────────────────────────────────────

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "list" => ControllerSchema {
            namespace: "campaigns",
            function: "list",
            description: "List campaigns matching the filter, newest-first by updated_at. Default excludes soft-deleted rows.",
            inputs: vec![FieldSchema {
                name: "filter",
                ty: TypeSchema::Ref("ListCampaignsFilter"),
                comment: "Optional filter (status, include_deleted). Defaults apply when omitted.",
                required: false,
            }],
            outputs: vec![FieldSchema {
                name: "campaigns",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("Campaign"))),
                comment: "Matching campaigns, newest-first.",
                required: true,
            }],
        },
        "get" => ControllerSchema {
            namespace: "campaigns",
            function: "get",
            description: "Fetch a single campaign by id. Returns null when the id is unknown or soft-deleted.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Campaign id (UUIDv4 string).",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "campaign",
                ty: TypeSchema::Ref("Campaign"),
                comment: "The campaign row, or null when unknown.",
                required: false,
            }],
        },
        "create" => ControllerSchema {
            namespace: "campaigns",
            function: "create",
            description: "Persist a new campaign row. Stamps id/created_at/updated_at, sets status=Draft, publishes CampaignDefined.",
            inputs: vec![FieldSchema {
                name: "request",
                ty: TypeSchema::Ref("CreateCampaignRequest"),
                comment: "Caller carries name/entity_binding/approval_policy. Status defaults to Draft.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "campaign",
                ty: TypeSchema::Ref("Campaign"),
                comment: "The persisted campaign with stamped id + initial Draft status.",
                required: true,
            }],
        },
        "update" => ControllerSchema {
            namespace: "campaigns",
            function: "update",
            description: "Apply a CampaignPatch to a live row. Status updates are NOT routed here — use pause/resume/archive/wind_down instead.",
            inputs: vec![FieldSchema {
                name: "request",
                ty: TypeSchema::Ref("UpdateCampaignRequest"),
                comment: "Campaign id + CampaignPatch (every field optional).",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "campaign",
                ty: TypeSchema::Ref("Campaign"),
                comment: "The updated campaign row.",
                required: true,
            }],
        },
        "delete" => ControllerSchema {
            namespace: "campaigns",
            function: "delete",
            description: "Soft-delete (sets deleted_at). Does NOT cascade-delete child workflows; ON DELETE SET NULL orphans them when the retention sweep hard-deletes later.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Campaign id to soft-delete.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "touched",
                ty: TypeSchema::Bool,
                comment: "True when a row was touched; false when id unknown or already deleted.",
                required: true,
            }],
        },
        "restore" => ControllerSchema {
            namespace: "campaigns",
            function: "restore",
            description: "Inverse of delete — clears deleted_at on a previously soft-deleted row.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Campaign id to restore.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "campaign",
                ty: TypeSchema::Ref("Campaign"),
                comment: "Restored campaign row, or null when the id wasn't soft-deleted / unknown.",
                required: false,
            }],
        },
        "pause" => ControllerSchema {
            namespace: "campaigns",
            function: "pause",
            description: "Transition Active → Paused. Cascades disable() to every child workflow.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Campaign id.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "campaign",
                ty: TypeSchema::Ref("Campaign"),
                comment: "Campaign with updated status. Errors with invalid_transition when current status doesn't allow the move.",
                required: true,
            }],
        },
        "resume" => ControllerSchema {
            namespace: "campaigns",
            function: "resume",
            description: "Transition Draft|Paused → Active. Cascades enable() to every child workflow.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Campaign id.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "campaign",
                ty: TypeSchema::Ref("Campaign"),
                comment: "Campaign with updated status.",
                required: true,
            }],
        },
        "wind_down" => ControllerSchema {
            namespace: "campaigns",
            function: "wind_down",
            description: "Transition to WoundDown — stop accepting new records; in-flight conversations continue.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Campaign id.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "campaign",
                ty: TypeSchema::Ref("Campaign"),
                comment: "Campaign with updated status.",
                required: true,
            }],
        },
        "archive" => ControllerSchema {
            namespace: "campaigns",
            function: "archive",
            description: "Terminal WoundDown → Archived transition. Disables every child workflow regardless of state.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Campaign id.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "campaign",
                ty: TypeSchema::Ref("Campaign"),
                comment: "Campaign with updated status.",
                required: true,
            }],
        },
        "add_workflow" => ControllerSchema {
            namespace: "campaigns",
            function: "add_workflow",
            description: "Link an existing standalone workflow to this campaign by setting workflows.campaign_id.",
            inputs: vec![
                FieldSchema {
                    name: "campaign_id",
                    ty: TypeSchema::String,
                    comment: "Campaign to link to.",
                    required: true,
                },
                FieldSchema {
                    name: "workflow_id",
                    ty: TypeSchema::String,
                    comment: "Workflow to link.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "touched",
                ty: TypeSchema::Bool,
                comment: "True when the link succeeded.",
                required: true,
            }],
        },
        "approvals_list_pending" => ControllerSchema {
            namespace: "campaigns",
            function: "approvals_list_pending",
            description: "F4-9: list pending approval-queue entries. Optional `campaign_id` filter.",
            inputs: vec![FieldSchema {
                name: "campaign_id",
                ty: TypeSchema::String,
                comment: "Optional campaign filter. Omit to list every campaign's pending drafts.",
                required: false,
            }],
            outputs: vec![FieldSchema {
                name: "entries",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("ApprovalEntry"))),
                comment: "Pending drafts, newest-first.",
                required: true,
            }],
        },
        "approvals_get" => ControllerSchema {
            namespace: "campaigns",
            function: "approvals_get",
            description: "F4-9: fetch a single approval row by id. Returns null when unknown.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Approval id (UUIDv4 string).",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "entry",
                ty: TypeSchema::Ref("ApprovalEntry"),
                comment: "The approval row or null when unknown.",
                required: false,
            }],
        },
        "approvals_approve" => ControllerSchema {
            namespace: "campaigns",
            function: "approvals_approve",
            description: "F4-9: approve a pending draft. Optional `edited_payload` swaps the action body before the re-issue path runs.",
            inputs: vec![
                FieldSchema {
                    name: "id",
                    ty: TypeSchema::String,
                    comment: "Approval id.",
                    required: true,
                },
                FieldSchema {
                    name: "edited_payload",
                    ty: TypeSchema::Json,
                    comment: "Optional replacement payload. Omit to approve with the original draft.",
                    required: false,
                },
                FieldSchema {
                    name: "decided_by",
                    ty: TypeSchema::String,
                    comment: "Optional actor label (defaults to \"user\").",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "entry",
                ty: TypeSchema::Ref("ApprovalEntry"),
                comment: "Approval row post-decision.",
                required: true,
            }],
        },
        "approvals_reject" => ControllerSchema {
            namespace: "campaigns",
            function: "approvals_reject",
            description: "F4-9: reject a pending draft. Does NOT re-issue.",
            inputs: vec![
                FieldSchema {
                    name: "id",
                    ty: TypeSchema::String,
                    comment: "Approval id.",
                    required: true,
                },
                FieldSchema {
                    name: "reason",
                    ty: TypeSchema::String,
                    comment: "Optional reason recorded on the row for the audit log.",
                    required: false,
                },
                FieldSchema {
                    name: "decided_by",
                    ty: TypeSchema::String,
                    comment: "Optional actor label (defaults to \"user\").",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "entry",
                ty: TypeSchema::Ref("ApprovalEntry"),
                comment: "Approval row post-decision.",
                required: true,
            }],
        },
        "approvals_batch_approve" => ControllerSchema {
            namespace: "campaigns",
            function: "approvals_batch_approve",
            description: "F4-9: approve N drafts in one call. Per-id failures are skipped + logged; never short-circuits the batch.",
            inputs: vec![
                FieldSchema {
                    name: "ids",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Approval ids to approve.",
                    required: true,
                },
                FieldSchema {
                    name: "decided_by",
                    ty: TypeSchema::String,
                    comment: "Optional actor label.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "entries",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("ApprovalEntry"))),
                comment: "Successfully-approved rows. May be shorter than input ids when some failed.",
                required: true,
            }],
        },
        "throttle_status" => ControllerSchema {
            namespace: "campaigns",
            function: "throttle_status",
            description: "F4-8 read-only throttle snapshot — current window's consumed/limit/remaining + next_window_at. Returns null when the campaign has no throttle configured.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Campaign id.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "snapshot",
                ty: TypeSchema::Ref("ThrottleSnapshot"),
                comment: "Current window snapshot or null when throttle is unset.",
                required: false,
            }],
        },
        "remove_workflow" => ControllerSchema {
            namespace: "campaigns",
            function: "remove_workflow",
            description: "Unlink a workflow (sets workflows.campaign_id = NULL). The workflow itself is not deleted.",
            inputs: vec![
                FieldSchema {
                    name: "campaign_id",
                    ty: TypeSchema::String,
                    comment: "Campaign currently linked.",
                    required: true,
                },
                FieldSchema {
                    name: "workflow_id",
                    ty: TypeSchema::String,
                    comment: "Workflow to unlink.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "touched",
                ty: TypeSchema::Bool,
                comment: "True when the unlink succeeded; false when the workflow wasn't linked or is unknown.",
                required: true,
            }],
        },
        _other => ControllerSchema {
            namespace: "campaigns",
            function: "unknown",
            description: "Unknown campaigns controller function.",
            inputs: vec![FieldSchema {
                name: "function",
                ty: TypeSchema::String,
                comment: "Unknown function requested for schema lookup.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "error",
                ty: TypeSchema::String,
                comment: "Lookup error details.",
                required: true,
            }],
        },
    }
}

// ── Handlers ───────────────────────────────────────────────────────────

fn handle_list(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let filter: ListCampaignsFilter = match params.get("filter") {
            Some(v) if !v.is_null() => parse_filter(v).map_err(|e| e.to_string())?,
            _ => ListCampaignsFilter::default(),
        };
        to_json(crate::openhuman::campaigns::rpc::campaigns_list(&config, filter).await?)
    })
}

fn handle_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::campaigns::rpc::campaigns_get(&config, id).await?)
    })
}

fn handle_create(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let req: CreateCampaignRequest = required_struct(&params, "request")?;
        to_json(crate::openhuman::campaigns::rpc::campaigns_create(&config, req).await?)
    })
}

fn handle_update(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let req: UpdateCampaignRequest = required_struct(&params, "request")?;
        to_json(crate::openhuman::campaigns::rpc::campaigns_update(&config, req).await?)
    })
}

fn handle_delete(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::campaigns::rpc::campaigns_delete(&config, id).await?)
    })
}

fn handle_restore(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::campaigns::rpc::campaigns_restore(&config, id).await?)
    })
}

fn handle_pause(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::campaigns::rpc::campaigns_pause(&config, id).await?)
    })
}

fn handle_resume(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::campaigns::rpc::campaigns_resume(&config, id).await?)
    })
}

fn handle_wind_down(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::campaigns::rpc::campaigns_wind_down(&config, id).await?)
    })
}

fn handle_archive(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::campaigns::rpc::campaigns_archive(&config, id).await?)
    })
}

fn handle_add_workflow(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let campaign_id = required_string(&params, "campaign_id")?;
        let workflow_id = required_string(&params, "workflow_id")?;
        to_json(
            crate::openhuman::campaigns::rpc::campaigns_add_workflow(
                &config,
                campaign_id,
                workflow_id,
            )
            .await?,
        )
    })
}

fn handle_remove_workflow(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let campaign_id = required_string(&params, "campaign_id")?;
        let workflow_id = required_string(&params, "workflow_id")?;
        to_json(
            crate::openhuman::campaigns::rpc::campaigns_remove_workflow(
                &config,
                campaign_id,
                workflow_id,
            )
            .await?,
        )
    })
}

fn handle_approvals_list_pending(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let campaign_id = params
            .get("campaign_id")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        to_json(
            crate::openhuman::campaigns::rpc::approvals_list_pending(&config, campaign_id).await?,
        )
    })
}

fn handle_approvals_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::campaigns::rpc::approvals_get(&config, id).await?)
    })
}

fn handle_approvals_approve(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        let edited_payload = params.get("edited_payload").cloned().filter(|v| !v.is_null());
        let decided_by = params
            .get("decided_by")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        to_json(
            crate::openhuman::campaigns::rpc::approvals_approve(
                &config,
                id,
                edited_payload,
                decided_by,
            )
            .await?,
        )
    })
}

fn handle_approvals_reject(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        let reason = params
            .get("reason")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let decided_by = params
            .get("decided_by")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        to_json(
            crate::openhuman::campaigns::rpc::approvals_reject(
                &config, id, reason, decided_by,
            )
            .await?,
        )
    })
}

fn handle_approvals_batch_approve(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let ids: Vec<String> = params
            .get("ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .ok_or_else(|| "missing required param `ids` (string array)".to_string())?;
        let decided_by = params
            .get("decided_by")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        to_json(
            crate::openhuman::campaigns::rpc::approvals_batch_approve(
                &config,
                ids,
                decided_by,
            )
            .await?,
        )
    })
}

fn handle_throttle_status(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::campaigns::rpc::campaigns_throttle_status(&config, id).await?)
    })
}

// ── helpers ────────────────────────────────────────────────────────────

fn parse_filter(value: &Value) -> Result<ListCampaignsFilter, serde_json::Error> {
    // The store-side `ListCampaignsFilter` isn't Deserialize (carries
    // a non-serde-friendly status field shape); decode an intermediate
    // wire shape and translate.
    #[derive(serde::Deserialize, Default)]
    struct Wire {
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        include_deleted: bool,
    }
    let wire: Wire = serde_json::from_value(value.clone())?;
    let status = wire.status.and_then(|s| match s.as_str() {
        "draft" => Some(CampaignStatus::Draft),
        "active" => Some(CampaignStatus::Active),
        "paused" => Some(CampaignStatus::Paused),
        "wound_down" => Some(CampaignStatus::WoundDown),
        "archived" => Some(CampaignStatus::Archived),
        _ => None,
    });
    Ok(ListCampaignsFilter {
        status,
        include_deleted: wire.include_deleted,
    })
}

fn required_string(params: &Map<String, Value>, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("missing required param `{key}`"))
}

fn required_struct<T: serde::de::DeserializeOwned>(
    params: &Map<String, Value>,
    key: &str,
) -> Result<T, String> {
    let raw = params
        .get(key)
        .cloned()
        .ok_or_else(|| format!("missing required param `{key}`"))?;
    serde_json::from_value(raw).map_err(|e| format!("invalid `{key}`: {e}"))
}

fn to_json<T: Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}
