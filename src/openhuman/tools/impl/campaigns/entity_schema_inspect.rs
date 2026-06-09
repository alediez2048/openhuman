//! `entity_schema_inspect` agent tool (F4-10).
//!
//! Read-only: opens an `EntityStore` adapter for the given binding
//! and returns the inferred field shape. The drafter calls this
//! mid-chat so it can mirror real columns back to the user before
//! emitting a `CampaignProposal` — never guess the schema.

use crate::openhuman::campaigns::entity_store::open_entity_store;
use crate::openhuman::campaigns::types::EntityRef;
use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct EntitySchemaInspectTool {
    config: Arc<Config>,
}

impl EntitySchemaInspectTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for EntitySchemaInspectTool {
    fn name(&self) -> &str {
        super::TOOL_ENTITY_SCHEMA_INSPECT
    }

    fn description(&self) -> &str {
        "Inspect the field shape of a campaign entity binding (Google Sheet or Attio object). \
         Returns `{ adapter, primary_field, fields: [{ key, label, kind, required }] }`. \
         Call this BEFORE proposing a campaign — never guess the schema. Input is the same \
         `entity_binding` shape carried on a Campaign: \
         `{ type: \"google_sheet\", spreadsheet_id, range }` or \
         `{ type: \"attio\", workspace_id, object_type }`."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "entity_binding": {
                    "type": "object",
                    "description": "EntityRef discriminated by `type`.",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["google_sheet", "attio"]
                        },
                        "spreadsheet_id": { "type": "string" },
                        "range": { "type": "string" },
                        "workspace_id": { "type": "string" },
                        "object_type": { "type": "string" }
                    },
                    "required": ["type"]
                }
            },
            "required": ["entity_binding"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some(binding_value) = args.get("entity_binding") else {
            return Ok(ToolResult::error(
                "entity_schema_inspect: `entity_binding` is required".to_string(),
            ));
        };
        let binding: EntityRef = match serde_json::from_value(binding_value.clone()) {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "entity_schema_inspect: invalid entity_binding: {e}"
                )));
            }
        };

        let store = match open_entity_store(&self.config, &binding) {
            Ok(s) => s,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "entity_schema_inspect: open adapter failed: {e:#}"
                )));
            }
        };

        let schema = match store.schema().await {
            Ok(s) => s,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "entity_schema_inspect: fetch schema failed: {e:#}"
                )));
            }
        };

        let body = json!({
            "adapter": store.adapter_id(),
            "primary_field": schema.primary_field,
            "fields": schema.fields,
        });
        Ok(ToolResult::success(body.to_string()))
    }
}
