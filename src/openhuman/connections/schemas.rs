//! Controller schemas + registry for the Connections domain.
//!
//! Phase 0 controllers:
//! - `connections_list` (P0-2) — unified read across all mechanisms.
//! - `connections_generic_http_create/_update/_delete` (P0-3) — Generic HTTP CRUD.
//! - `connections_test` (P0-3) — connectivity probe (stub in P0-3; real probe P0-3a).

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::connections::types::{
    ConnectionsListRequest, CreateGenericHttpRequest, McpAddRequest, UpdateGenericHttpRequest,
};
use crate::rpc::RpcOutcome;
use serde::Serialize;
use serde_json::{Map, Value};

/// All controller schemas declared by the connections domain.
pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list"),
        schemas("generic_http_create"),
        schemas("generic_http_update"),
        schemas("generic_http_delete"),
        schemas("generic_http_get"),
        schemas("test"),
        schemas("mcp_add"),
        schemas("mcp_remove"),
        schemas("mcp_test"),
        schemas("mcp_orphans_list"),
        schemas("mcp_orphans_migrate"),
    ]
}

/// All controllers (schema + handler) registered by the connections domain.
pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list"),
            handler: handle_list,
        },
        RegisteredController {
            schema: schemas("generic_http_create"),
            handler: handle_generic_http_create,
        },
        RegisteredController {
            schema: schemas("generic_http_update"),
            handler: handle_generic_http_update,
        },
        RegisteredController {
            schema: schemas("generic_http_delete"),
            handler: handle_generic_http_delete,
        },
        RegisteredController {
            schema: schemas("generic_http_get"),
            handler: handle_generic_http_get,
        },
        RegisteredController {
            schema: schemas("test"),
            handler: handle_test,
        },
        RegisteredController {
            schema: schemas("mcp_add"),
            handler: handle_mcp_add,
        },
        RegisteredController {
            schema: schemas("mcp_remove"),
            handler: handle_mcp_remove,
        },
        RegisteredController {
            schema: schemas("mcp_test"),
            handler: handle_mcp_test,
        },
        RegisteredController {
            schema: schemas("mcp_orphans_list"),
            handler: handle_mcp_orphans_list,
        },
        RegisteredController {
            schema: schemas("mcp_orphans_migrate"),
            handler: handle_mcp_orphans_migrate,
        },
    ]
}

pub fn all_connections_controller_schemas() -> Vec<ControllerSchema> {
    all_controller_schemas()
}

pub fn all_connections_registered_controllers() -> Vec<RegisteredController> {
    all_registered_controllers()
}

/// Schema definition for one controller function in the connections namespace.
pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "list" => ControllerSchema {
            namespace: "connections",
            function: "list",
            description: "List every connection across all 6 mechanisms with optional kind_filter + case-insensitive search.",
            inputs: vec![
                FieldSchema {
                    name: "kind_filter",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::Enum {
                            variants: vec![
                                "composio", "channel", "webview", "builtin", "mcp", "generic_http",
                            ],
                        },
                    )))),
                    comment: "Restrict the result to one or more connection kinds.",
                    required: false,
                },
                FieldSchema {
                    name: "search",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Case-insensitive substring matched against display_name.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "response",
                ty: TypeSchema::Ref("ConnectionsListResponse"),
                comment: "Aggregated, filtered list of connections + the wall-clock timestamp.",
                required: true,
            }],
        },
        "generic_http_create" => ControllerSchema {
            namespace: "connections",
            function: "generic_http_create",
            description: "Register a new Generic HTTP connection. Credential is encrypted via security/secrets before persistence.",
            inputs: vec![FieldSchema {
                name: "request",
                ty: TypeSchema::Ref("CreateGenericHttpRequest"),
                comment: "Cleartext credential is in-memory only and never persisted in this shape.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "connection",
                ty: TypeSchema::Ref("GenericHttpConnection"),
                comment: "Persisted connection. secret_ref carries the enc2:<hex> ciphertext; no cleartext.",
                required: true,
            }],
        },
        "generic_http_update" => ControllerSchema {
            namespace: "connections",
            function: "generic_http_update",
            description: "Update a Generic HTTP connection. None-valued fields keep the existing value. auth_credential = Some rotates the secret.",
            inputs: vec![
                FieldSchema {
                    name: "id",
                    ty: TypeSchema::String,
                    comment: "Identifier of the Generic HTTP connection to update.",
                    required: true,
                },
                FieldSchema {
                    name: "request",
                    ty: TypeSchema::Ref("UpdateGenericHttpRequest"),
                    comment: "Partial update payload.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "connection",
                ty: TypeSchema::Ref("GenericHttpConnection"),
                comment: "Updated connection.",
                required: true,
            }],
        },
        "generic_http_delete" => ControllerSchema {
            namespace: "connections",
            function: "generic_http_delete",
            description: "Delete a Generic HTTP connection. Idempotent — returns removed=false when the id was unknown.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Identifier of the Generic HTTP connection to delete.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "removed",
                ty: TypeSchema::Bool,
                comment: "True when a row was removed; false when the id was unknown.",
                required: true,
            }],
        },
        "test" => ControllerSchema {
            namespace: "connections",
            function: "test",
            description: "Best-effort connectivity probe. Phase 0 stub returns ok=true if the row exists; the real HEAD/OPTIONS/GET probe lands in P0-3a.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Identifier of the Generic HTTP connection to probe.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Ref("TestProbeResult"),
                comment: "Structured probe outcome — failures return ok=false rather than an error.",
                required: true,
            }],
        },
        "mcp_add" => ControllerSchema {
            namespace: "connections",
            function: "mcp_add",
            description: "Register a new MCP server in config.mcp_client.servers and persist the TOML. Aggregator picks up the new server on the next connections_list refresh.",
            inputs: vec![FieldSchema {
                name: "request",
                ty: TypeSchema::Ref("McpAddRequest"),
                comment: "Server metadata + transport (HTTP endpoint or stdio command) + optional auth.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "server",
                ty: TypeSchema::Ref("McpServerConfig"),
                comment: "Canonical persisted McpServerConfig.",
                required: true,
            }],
        },
        "generic_http_get" => ControllerSchema {
            namespace: "connections",
            function: "generic_http_get",
            description: "Fetch the full saved GenericHttpConnection row by id. The manage modal calls this so the form populates with real persisted values rather than a frontend-constructed stub.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Identifier of the Generic HTTP connection to fetch.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "connection",
                ty: TypeSchema::Ref("GenericHttpConnection"),
                comment: "The full row, or null when the id is unknown.",
                required: false,
            }],
        },
        "mcp_test" => ControllerSchema {
            namespace: "connections",
            function: "mcp_test",
            description: "Real MCP connectivity probe — calls initialize on the server and records the outcome in the verification cache. 15s timeout.",
            inputs: vec![FieldSchema {
                name: "server_id",
                ty: TypeSchema::String,
                comment: "MCP server name to probe.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Ref("TestProbeResult"),
                comment: "Probe outcome — failures return ok=false with a reason rather than an error.",
                required: true,
            }],
        },
        "mcp_remove" => ControllerSchema {
            namespace: "connections",
            function: "mcp_remove",
            description: "Remove an MCP server by name from config.mcp_client.servers. Idempotent — returns removed=false when the name was unknown.",
            inputs: vec![FieldSchema {
                name: "name",
                ty: TypeSchema::String,
                comment: "Server name (case-insensitive match against config.mcp_client.servers[].name).",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "removed",
                ty: TypeSchema::Bool,
                comment: "True when an entry was removed.",
                required: true,
            }],
        },
        "mcp_orphans_list" => ControllerSchema {
            namespace: "connections",
            function: "mcp_orphans_list",
            description: "F-18 Part 3: scan ~/.openhuman/users/*/config.toml for MCP servers registered under a previous-session user dir. Returns the orphan inventory so the /connections UI can surface a 'restore previous-session credentials' banner. Bearer tokens are redacted; the real value never crosses this RPC boundary.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "listing",
                ty: TypeSchema::Ref("McpOrphanListing"),
                comment: "Orphan inventory + scan diagnostics.",
                required: true,
            }],
        },
        "mcp_orphans_migrate" => ControllerSchema {
            namespace: "connections",
            function: "mcp_orphans_migrate",
            description: "F-18 Part 3: copy one orphan MCP server from a previous-session user's config into the active user's config. Reads the source bearer token server-side. Does NOT delete from the source.",
            inputs: vec![
                FieldSchema {
                    name: "source_user_id",
                    ty: TypeSchema::String,
                    comment: "User-dir name (the SHA-style id) the orphan currently lives under.",
                    required: true,
                },
                FieldSchema {
                    name: "server_name",
                    ty: TypeSchema::String,
                    comment: "Case-insensitive match against the source's mcp_client.servers[].name.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "server",
                ty: TypeSchema::Ref("McpServerConfig"),
                comment: "The migrated McpServerConfig (as persisted in the active user's config).",
                required: true,
            }],
        },
        _other => ControllerSchema {
            namespace: "connections",
            function: "unknown",
            description: "Unknown connections controller function.",
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
        let req: ConnectionsListRequest = serde_json::from_value(Value::Object(params))
            .map_err(|e| format!("invalid connections_list request: {e}"))?;
        to_json(crate::openhuman::connections::rpc::connections_list(&config, req).await?)
    })
}

fn handle_generic_http_create(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let req: CreateGenericHttpRequest = params
            .get("request")
            .cloned()
            .ok_or_else(|| "missing required param 'request'".to_string())
            .and_then(|v| {
                serde_json::from_value(v).map_err(|e| format!("invalid 'request': {e}"))
            })?;
        to_json(
            crate::openhuman::connections::rpc::connections_generic_http_create(&config, req)
                .await?,
        )
    })
}

fn handle_generic_http_update(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param 'id'".to_string())?
            .to_string();
        let req: UpdateGenericHttpRequest = params
            .get("request")
            .cloned()
            .ok_or_else(|| "missing required param 'request'".to_string())
            .and_then(|v| {
                serde_json::from_value(v).map_err(|e| format!("invalid 'request': {e}"))
            })?;
        to_json(
            crate::openhuman::connections::rpc::connections_generic_http_update(&config, &id, req)
                .await?,
        )
    })
}

fn handle_generic_http_delete(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param 'id'".to_string())?
            .to_string();
        to_json(
            crate::openhuman::connections::rpc::connections_generic_http_delete(&config, &id)
                .await?,
        )
    })
}

fn handle_test(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param 'id'".to_string())?
            .to_string();
        to_json(crate::openhuman::connections::rpc::connections_test(&config, &id).await?)
    })
}

fn handle_generic_http_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param 'id'".to_string())?
            .to_string();
        to_json(
            crate::openhuman::connections::rpc::connections_generic_http_get(&config, &id).await?,
        )
    })
}

fn handle_mcp_test(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let server_id = params
            .get("server_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param 'server_id'".to_string())?
            .to_string();
        to_json(
            crate::openhuman::connections::rpc::connections_mcp_test(&config, &server_id).await?,
        )
    })
}

fn handle_mcp_add(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let req: crate::openhuman::connections::types::McpAddRequest = params
            .get("request")
            .cloned()
            .ok_or_else(|| "missing required param 'request'".to_string())
            .and_then(|v| {
                serde_json::from_value(v).map_err(|e| format!("invalid 'request': {e}"))
            })?;
        to_json(crate::openhuman::connections::rpc::connections_mcp_add(&config, req).await?)
    })
}

fn handle_mcp_remove(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param 'name'".to_string())?
            .to_string();
        to_json(crate::openhuman::connections::rpc::connections_mcp_remove(&config, &name).await?)
    })
}

fn handle_mcp_orphans_list(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::openhuman::connections::rpc::connections_mcp_orphans_list(&config).await?,
        )
    })
}

fn handle_mcp_orphans_migrate(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let source_user_id = params
            .get("source_user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param 'source_user_id'".to_string())?
            .to_string();
        let server_name = params
            .get("server_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param 'server_name'".to_string())?
            .to_string();
        to_json(
            crate::openhuman::connections::rpc::connections_mcp_orphans_migrate(
                &config,
                &source_user_id,
                &server_name,
            )
            .await?,
        )
    })
}

fn to_json<T: Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}
