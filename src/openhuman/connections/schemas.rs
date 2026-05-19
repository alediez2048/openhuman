//! Controller schemas + registry for the Connections domain.
//!
//! Phase 0 / P0-2 registers `connections_list` (read-only union across all
//! six mechanisms). P0-3 will add `connections_generic_http_create/_update/
//! _delete` and `connections_test`.

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::connections::types::ConnectionsListRequest;
use crate::rpc::RpcOutcome;
use serde::Serialize;
use serde_json::{Map, Value};

/// All controller schemas declared by the connections domain.
pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![schemas("list")]
}

/// All controllers (schema + handler) registered by the connections domain.
pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![RegisteredController {
        schema: schemas("list"),
        handler: handle_list,
    }]
}

/// Convenience: same as [`all_controller_schemas`] but uniquely named for
/// the `src/core/all.rs` aggregation point.
pub fn all_connections_controller_schemas() -> Vec<ControllerSchema> {
    all_controller_schemas()
}

/// Convenience: same as [`all_registered_controllers`] but uniquely named for
/// the `src/core/all.rs` aggregation point.
pub fn all_connections_registered_controllers() -> Vec<RegisteredController> {
    all_registered_controllers()
}

/// Schema definition for one controller function in the connections namespace.
pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "list" => ControllerSchema {
            namespace: "connections",
            function: "list",
            description: "List every connection across all 6 mechanisms (composio, channel, webview, builtin, mcp, generic_http). Supports optional kind_filter and case-insensitive substring search.",
            inputs: vec![
                FieldSchema {
                    name: "kind_filter",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::Enum {
                            variants: vec![
                                "composio",
                                "channel",
                                "webview",
                                "builtin",
                                "mcp",
                                "generic_http",
                            ],
                        },
                    )))),
                    comment: "Restrict the result to one or more connection kinds. Empty/missing means all kinds.",
                    required: false,
                },
                FieldSchema {
                    name: "search",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Optional case-insensitive substring matched against display_name.",
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

fn handle_list(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let req: ConnectionsListRequest = serde_json::from_value(Value::Object(params))
            .map_err(|e| format!("invalid connections_list request: {e}"))?;
        to_json(crate::openhuman::connections::rpc::connections_list(&config, req).await?)
    })
}

fn to_json<T: Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}
