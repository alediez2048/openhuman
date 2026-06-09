//! F3-3 — `browser_act` tool (Phase 3.1 MVP — fast path only).
//!
//! "Do this thing on the page." Phase 3.1 ships the **fast path**:
//! the LLM passes `element_id` (from the latest `browser_observe`
//! snapshot) + a verb (`click` / `type` / `select` / `scroll` /
//! `navigate`), and the tool dispatches the matching CDP primitive
//! directly.
//!
//! The Stagehand-style NL path ("click the Save button") needs an
//! intent-LLM translation step and is the F3-3 follow-up. Until
//! then, the agent calls `browser_observe` first + reasons about
//! ids itself.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::openhuman::browser_agent::cdp::types::{MouseButton, TypeOptions};
use crate::openhuman::browser_agent::perceive::{snapshot, ActionableElement, SnapshotOptions};
use crate::openhuman::browser_agent::registry::SessionRegistry;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};

pub struct BrowserActTool;

impl BrowserActTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BrowserActTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BrowserActTool {
    fn name(&self) -> &str {
        super::constants::TOOL_BROWSER_ACT
    }

    fn description(&self) -> &str {
        "Perform an action on the current browser page. Phase 3.1 ships the fast path: \
         pass `verb` (\"click\" / \"type\" / \"scroll\" / \"navigate\") + the relevant args. \
         For `click` and `type`, supply `element_id` from the most recent `browser_observe` \
         snapshot. Returns the post-action page state summary so you can chain.\n\
         \n\
         Examples:\n\
         - `{verb:\"click\", element_id:3}` — click element [3].\n\
         - `{verb:\"type\", element_id:7, text:\"hello\"}` — type into input [7].\n\
         - `{verb:\"navigate\", url:\"https://example.com\"}`\n\
         - `{verb:\"scroll\", dy:500}` — scroll down 500px."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "user_id": { "type": "string", "description": "Injected by the executor." },
                "run_id": { "type": "string", "description": "Injected by the executor." },
                "verb": {
                    "type": "string",
                    "enum": ["click", "type", "scroll", "navigate"]
                },
                "element_id": {
                    "type": "integer",
                    "description": "Required for click / type. Element id from the latest browser_observe."
                },
                "text": {
                    "type": "string",
                    "description": "Required for verb=type."
                },
                "url": {
                    "type": "string",
                    "description": "Required for verb=navigate."
                },
                "dy": {
                    "type": "number",
                    "description": "Required for verb=scroll. Positive = down, negative = up."
                }
            },
            "required": ["user_id", "run_id", "verb"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        // Writes to the live page surface — clicks + types are user-
        // facing side effects. Write tier matches the safety preamble
        // (F3-6) gates.
        PermissionLevel::Write
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolResult> {
        let Some(user_id) = args.get("user_id").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("browser_act: `user_id` required"));
        };
        let Some(run_id) = args.get("run_id").and_then(|v| v.as_str()) else {
            return Ok(ToolResult::error("browser_act: `run_id` required"));
        };
        let verb = args.get("verb").and_then(|v| v.as_str()).unwrap_or("");
        let Some(session) =
            SessionRegistry::instance().get(&user_id.to_string(), &run_id.to_string())
        else {
            return Ok(ToolResult::error(
                "browser_act: no active session for this run",
            ));
        };

        let outcome = match verb {
            "click" => {
                let Some(eid) = args.get("element_id").and_then(|v| v.as_u64()) else {
                    return Ok(ToolResult::error(
                        "browser_act(click): `element_id` required",
                    ));
                };
                let snap = match snapshot(&session, SnapshotOptions::default()).await {
                    Ok(s) => s,
                    Err(e) => return Ok(ToolResult::error(format!("browser_act: snapshot: {e}"))),
                };
                match find_element(&snap.elements, eid as u32) {
                    Some(el) => {
                        // Click the rect center.
                        let cx = el.bounds.x + el.bounds.width / 2.0;
                        let cy = el.bounds.y + el.bounds.height / 2.0;
                        if let Err(e) = session.click_at(cx, cy, MouseButton::Left).await {
                            return Ok(ToolResult::error(format!("browser_act(click): {e}")));
                        }
                        format!("clicked [{eid}] {}", el.label)
                    }
                    None => {
                        return Ok(ToolResult::error(format!(
                            "browser_act(click): element [{eid}] not in latest snapshot"
                        )));
                    }
                }
            }
            "type" => {
                let Some(eid) = args.get("element_id").and_then(|v| v.as_u64()) else {
                    return Ok(ToolResult::error(
                        "browser_act(type): `element_id` required",
                    ));
                };
                let Some(text) = args.get("text").and_then(|v| v.as_str()) else {
                    return Ok(ToolResult::error("browser_act(type): `text` required"));
                };
                let snap = match snapshot(&session, SnapshotOptions::default()).await {
                    Ok(s) => s,
                    Err(e) => return Ok(ToolResult::error(format!("browser_act: snapshot: {e}"))),
                };
                match find_element(&snap.elements, eid as u32) {
                    Some(el) => {
                        // Click to focus, then type humanized.
                        let cx = el.bounds.x + el.bounds.width / 2.0;
                        let cy = el.bounds.y + el.bounds.height / 2.0;
                        if let Err(e) = session.click_at(cx, cy, MouseButton::Left).await {
                            return Ok(ToolResult::error(format!("browser_act(type focus): {e}")));
                        }
                        if let Err(e) = session.type_text(text, TypeOptions::default()).await {
                            return Ok(ToolResult::error(format!("browser_act(type): {e}")));
                        }
                        format!("typed {} chars into [{eid}] {}", text.len(), el.label)
                    }
                    None => {
                        return Ok(ToolResult::error(format!(
                            "browser_act(type): element [{eid}] not in latest snapshot"
                        )));
                    }
                }
            }
            "scroll" => {
                let dy = args.get("dy").and_then(|v| v.as_f64()).unwrap_or(0.0);
                if let Err(e) = session.scroll(0.0, dy).await {
                    return Ok(ToolResult::error(format!("browser_act(scroll): {e}")));
                }
                format!("scrolled dy={dy}")
            }
            "navigate" => {
                let Some(url) = args.get("url").and_then(|v| v.as_str()) else {
                    return Ok(ToolResult::error("browser_act(navigate): `url` required"));
                };
                if let Err(e) = session.navigate(url).await {
                    return Ok(ToolResult::error(format!("browser_act(navigate): {e}")));
                }
                format!("navigated to {url}")
            }
            other => {
                return Ok(ToolResult::error(format!(
                    "browser_act: unknown verb `{other}` (expected click/type/scroll/navigate)"
                )));
            }
        };

        let after_url = session.url().await.unwrap_or_default();
        let markdown = format!("→ {outcome}");
        let body = json!({
            "status": "ok",
            "observed_change": {
                "url": after_url,
                "summary": outcome,
            }
        });
        Ok(ToolResult::success_with_markdown(body, markdown))
    }
}

fn find_element(elements: &[ActionableElement], id: u32) -> Option<&ActionableElement> {
    elements.iter().find(|e| e.id == id)
}
