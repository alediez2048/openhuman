//! F3-3 — `browser_extract` tool (Phase 3.1 MVP — deterministic path).
//!
//! Pull labelled values out of the current page. Phase 3.1 ships the
//! deterministic path: pass `element_ids` (from the latest
//! `browser_observe`), and the tool returns each element's
//! `{ id, role, label, value, href }` slot. Optional `text_pattern`
//! filter narrows the snapshot's text excerpt.
//!
//! The Stagehand-style "give me a JSON value matching this schema"
//! path needs an LLM extraction loop and is the F3-3 follow-up.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::openhuman::browser_agent::perceive::{snapshot, SnapshotOptions};
use crate::openhuman::browser_agent::registry::SessionRegistry;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};

pub struct BrowserExtractTool;

impl BrowserExtractTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BrowserExtractTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BrowserExtractTool {
    fn name(&self) -> &str {
        super::constants::TOOL_BROWSER_EXTRACT
    }

    fn description(&self) -> &str {
        "Extract values from the current browser page. Phase 3.1 surface: pass `element_ids` \
         (numbers from the latest `browser_observe` snapshot) and the tool returns each \
         element's label, value (input value), and href (for links). Optional `text_pattern` \
         (regex) filters the page's text excerpt to lines that match. Returns JSON: \
         `{ elements: [{id, label, role, value, href}], text_matches: [...] }`."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "user_id": { "type": "string", "description": "Injected by the executor." },
                "run_id": { "type": "string", "description": "Injected by the executor." },
                "element_ids": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Element ids from the latest browser_observe."
                },
                "text_pattern": {
                    "type": "string",
                    "description": "Optional regex; lines from the page text matching this are returned in `text_matches`."
                }
            },
            "required": ["user_id", "run_id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolResult> {
        let Some(user_id) = args.get("user_id").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("browser_extract: `user_id` required"));
        };
        let Some(run_id) = args.get("run_id").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("browser_extract: `run_id` required"));
        };
        let Some(session) =
            SessionRegistry::instance().get(&user_id.to_string(), &run_id.to_string())
        else {
            return Ok(ToolResult::error(
                "browser_extract: no active session for this run",
            ));
        };

        // F3-6 chunk 3: wall-clock cost cap.
        if let Some(short_circuit) =
            super::observe::check_wall_clock_cap(user_id, run_id, "browser_extract", &args)
        {
            return Ok(short_circuit);
        }

        let snap = match snapshot(&session, SnapshotOptions::default()).await {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::error(format!("browser_extract: {e}"))),
        };

        let ids: Vec<u64> = args
            .get("element_ids")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default();
        let mut extracted = Vec::with_capacity(ids.len());
        for id in &ids {
            let want = *id as u32;
            if let Some(el) = snap.elements.iter().find(|e| e.id == want) {
                extracted.push(json!({
                    "id": el.id,
                    "role": el.role.render_label(),
                    "label": el.label,
                    "value": el.attributes.get("value"),
                    "href": el.attributes.get("href"),
                }));
            } else {
                extracted.push(json!({ "id": id, "error": "not in snapshot" }));
            }
        }

        let text_matches: Vec<String> = match args.get("text_pattern").and_then(|v| v.as_str()) {
            Some(pat) => match regex::Regex::new(pat) {
                Ok(re) => snap
                    .text_content
                    .lines()
                    .filter(|line| re.is_match(line))
                    .take(50)
                    .map(String::from)
                    .collect(),
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "browser_extract: invalid text_pattern regex: {e}"
                    )));
                }
            },
            None => Vec::new(),
        };

        let body = json!({
            "elements": extracted,
            "text_matches": text_matches,
            "url": snap.url,
        });
        let markdown = render_extract_markdown(&body);
        // F3-6 chunk 2: audit-log the extract call with its result
        // shape so the run-detail UI shows what the agent pulled out.
        super::observe::emit_audit(
            user_id,
            run_id,
            "browser_extract",
            &args,
            &format!(
                "extracted {} elements + {} text matches from {}",
                extracted.len(),
                text_matches.len(),
                snap.url,
            ),
        );
        Ok(ToolResult::success_with_markdown(body, markdown))
    }
}

fn render_extract_markdown(body: &Value) -> String {
    let mut out = String::new();
    if let Some(url) = body.get("url").and_then(|v| v.as_str()) {
        out.push_str(&format!("URL: {url}\n\n"));
    }
    if let Some(elements) = body.get("elements").and_then(|v| v.as_array()) {
        if !elements.is_empty() {
            out.push_str("Extracted:\n");
            for el in elements {
                let id = el.get("id").map(|v| v.to_string()).unwrap_or_default();
                let label = el
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(no label)");
                let role = el.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                let value = el.get("value").and_then(|v| v.as_str());
                let href = el.get("href").and_then(|v| v.as_str());
                out.push_str(&format!("- [{id}] {role} \"{label}\""));
                if let Some(v) = value {
                    out.push_str(&format!(" value=\"{v}\""));
                }
                if let Some(h) = href {
                    out.push_str(&format!(" href={h}"));
                }
                out.push('\n');
            }
        }
    }
    if let Some(matches) = body.get("text_matches").and_then(|v| v.as_array()) {
        if !matches.is_empty() {
            out.push_str("\nText matches:\n");
            for m in matches {
                if let Some(line) = m.as_str() {
                    out.push_str(&format!("- {line}\n"));
                }
            }
        }
    }
    out
}
