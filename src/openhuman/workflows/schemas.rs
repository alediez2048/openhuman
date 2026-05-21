//! Controller schemas + registry for the Workflows domain.
//!
//! Phase 1 / F-2 ships seven mutating + read controllers:
//! - `workflows_list` — filtered list view.
//! - `workflows_get` — single-row fetch (null when unknown).
//! - `workflows_create` — persist + publish `WorkflowDefined`.
//! - `workflows_update` — partial update via `WorkflowPatch`.
//! - `workflows_delete` — hard-delete with FK cascade.
//! - `workflows_enable` / `workflows_disable` — flip the `enabled` bit.
//!
//! F-5 / F-7 / F-8 each layer in additional controllers
//! (`workflows_list_starter_templates`, `workflows_run_now`,
//! `workflows_cancel_run`, `workflows_list_runs`, `workflows_get_run`).

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::workflows::types::{
    CreateWorkflowRequest, ListFilter, ListStarterTemplatesRequest, UpdateWorkflowRequest,
};
use crate::rpc::RpcOutcome;
use serde::Serialize;
use serde_json::{Map, Value};

/// All controller schemas declared by the workflows domain (F-2 + F-5).
pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list"),
        schemas("get"),
        schemas("create"),
        schemas("update"),
        schemas("delete"),
        schemas("enable"),
        schemas("disable"),
        schemas("list_starter_templates"),
    ]
}

/// All controllers (schema + handler) registered by the workflows
/// domain (F-2).
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
            schema: schemas("enable"),
            handler: handle_enable,
        },
        RegisteredController {
            schema: schemas("disable"),
            handler: handle_disable,
        },
        RegisteredController {
            schema: schemas("list_starter_templates"),
            handler: handle_list_starter_templates,
        },
    ]
}

/// Alias used by `core/all.rs` to compose every domain's schemas.
pub fn all_workflows_controller_schemas() -> Vec<ControllerSchema> {
    all_controller_schemas()
}

/// Alias used by `core/all.rs` to compose every domain's controllers.
pub fn all_workflows_registered_controllers() -> Vec<RegisteredController> {
    all_registered_controllers()
}

/// Schema definition for one controller function in the workflows namespace.
pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "list" => ControllerSchema {
            namespace: "workflows",
            function: "list",
            description: "List workflows matching the filter, sorted by updated_at DESC.",
            inputs: vec![FieldSchema {
                name: "filter",
                ty: TypeSchema::Ref("ListFilter"),
                comment: "Optional list-view filter (enabled, health_state, search). Defaults apply when omitted.",
                required: false,
            }],
            outputs: vec![FieldSchema {
                name: "workflows",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("Workflow"))),
                comment: "Workflows matching the filter, newest-first by updated_at.",
                required: true,
            }],
        },
        "get" => ControllerSchema {
            namespace: "workflows",
            function: "get",
            description: "Fetch a single workflow by id. Returns null when the id is unknown.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Workflow id (UUIDv7).",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "workflow",
                ty: TypeSchema::Ref("Workflow"),
                comment: "The workflow row, or null when the id is unknown.",
                required: false,
            }],
        },
        "create" => ControllerSchema {
            namespace: "workflows",
            function: "create",
            description: "Persist a new workflow row. Stamps id/created_at/updated_at, sets enabled=false, computes initial health, publishes WorkflowDefined.",
            inputs: vec![FieldSchema {
                name: "request",
                ty: TypeSchema::Ref("CreateWorkflowRequest"),
                comment: "Caller carries `origin` (ADR-018). `Imported` is rejected in Phase 1.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "workflow",
                ty: TypeSchema::Ref("Workflow"),
                comment: "The persisted workflow, including stamped id + initial health.",
                required: true,
            }],
        },
        "update" => ControllerSchema {
            namespace: "workflows",
            function: "update",
            description: "Apply a partial update to a workflow. None-valued patch fields are preserved. Recomputes health + bumps updated_at; publishes WorkflowUpdated.",
            inputs: vec![FieldSchema {
                name: "request",
                ty: TypeSchema::Ref("UpdateWorkflowRequest"),
                comment: "Workflow id + WorkflowPatch (every field optional).",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "workflow",
                ty: TypeSchema::Ref("Workflow"),
                comment: "The updated workflow row.",
                required: true,
            }],
        },
        "delete" => ControllerSchema {
            namespace: "workflows",
            function: "delete",
            description: "Hard-delete a workflow. FK cascade drops workflow_runs + workflow_run_steps. Idempotent — returns removed=false when the id was unknown.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Workflow id to delete.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "removed",
                ty: TypeSchema::Bool,
                comment: "True when a row was removed; false when the id was unknown.",
                required: true,
            }],
        },
        "enable" => ControllerSchema {
            namespace: "workflows",
            function: "enable",
            description: "Flip enabled = true on a workflow. Publishes WorkflowEnabled only on a real transition.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Workflow id to enable.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "workflow",
                ty: TypeSchema::Ref("Workflow"),
                comment: "The workflow row after the toggle.",
                required: true,
            }],
        },
        "disable" => ControllerSchema {
            namespace: "workflows",
            function: "disable",
            description: "Flip enabled = false on a workflow. Publishes WorkflowDisabled only on a real transition.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Workflow id to disable.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "workflow",
                ty: TypeSchema::Ref("Workflow"),
                comment: "The workflow row after the toggle.",
                required: true,
            }],
        },
        "list_starter_templates" => ControllerSchema {
            namespace: "workflows",
            function: "list_starter_templates",
            description: "Read-only catalog of bundled RU-* starter templates filtered by phase and deduplicated against the user's existing Seed{template_id} workflows. Each row carries missing_connections computed against the live aggregator snapshot.",
            inputs: vec![FieldSchema {
                name: "request",
                ty: TypeSchema::Ref("ListStarterTemplatesRequest"),
                comment: "Optional phase override (defaults to the current Phase server-side).",
                required: false,
            }],
            outputs: vec![FieldSchema {
                name: "templates",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("StarterTemplateView"))),
                comment: "Catalog rows the F-6 UI renders. F-6's [Add] flow passes raw_payload back into workflows_create.",
                required: true,
            }],
        },
        _other => ControllerSchema {
            namespace: "workflows",
            function: "unknown",
            description: "Unknown workflows controller function.",
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

// ── Handlers ────────────────────────────────────────────────────────────

fn handle_list(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        // The `filter` param is optional — both unset and explicit `{}`
        // are valid and map to the default ListFilter.
        let filter: ListFilter = match params.get("filter") {
            Some(v) if !v.is_null() => {
                serde_json::from_value(v.clone()).map_err(|e| format!("invalid `filter`: {e}"))?
            }
            _ => ListFilter::default(),
        };
        to_json(crate::openhuman::workflows::rpc::workflows_list(&config, filter).await?)
    })
}

fn handle_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::workflows::rpc::workflows_get(&config, id).await?)
    })
}

fn handle_create(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let req: CreateWorkflowRequest = required_struct(&params, "request")?;
        to_json(crate::openhuman::workflows::rpc::workflows_create(&config, req).await?)
    })
}

fn handle_update(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let req: UpdateWorkflowRequest = required_struct(&params, "request")?;
        to_json(crate::openhuman::workflows::rpc::workflows_update(&config, req).await?)
    })
}

fn handle_delete(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::workflows::rpc::workflows_delete(&config, id).await?)
    })
}

fn handle_enable(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::workflows::rpc::workflows_enable(&config, id).await?)
    })
}

fn handle_disable(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = required_string(&params, "id")?;
        to_json(crate::openhuman::workflows::rpc::workflows_disable(&config, id).await?)
    })
}

fn handle_list_starter_templates(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        // The `request` param is optional — both unset and `{}` map to
        // the default `ListStarterTemplatesRequest` (phase=None).
        let req: ListStarterTemplatesRequest = match params.get("request") {
            Some(v) if !v.is_null() => {
                serde_json::from_value(v.clone()).map_err(|e| format!("invalid `request`: {e}"))?
            }
            _ => ListStarterTemplatesRequest::default(),
        };
        to_json(
            crate::openhuman::workflows::rpc::workflows_list_starter_templates(&config, req)
                .await?,
        )
    })
}

// ── helpers ─────────────────────────────────────────────────────────────

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
