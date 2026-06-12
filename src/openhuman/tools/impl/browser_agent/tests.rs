//! F3-3 — tool-layer tests against MockTransport + SessionRegistry.
//!
//! Each test installs a fresh `CdpSession` (backed by a `MockTransport`
//! pre-loaded with the expected CDP responses) into the global
//! `SessionRegistry` under a **unique** `(user_id, run_id)` so parallel
//! cargo-test execution doesn't race. F3-4 will exercise the
//! `open_or_attach` path end-to-end against live CEF; these tests
//! pin the tool surface contract.

use std::sync::Arc;

use serde_json::json;

use super::{BrowserActTool, BrowserExtractTool, BrowserObserveTool};
use crate::openhuman::browser_agent::cdp::session::CdpSession;
use crate::openhuman::browser_agent::cdp::transport::{CdpTransport, MockTransport};
use crate::openhuman::browser_agent::registry::SessionRegistry;
use crate::openhuman::tools::traits::Tool;

/// Bundle a session backed by `mock` into the registry under
/// `(user_id, run_id)`. Returns the mock so the test can queue
/// additional expectations or assert observed calls.
async fn install_session(
    user_id: &str,
    run_id: &str,
    mock: Arc<MockTransport>,
) -> Arc<MockTransport> {
    let transport = Arc::clone(&mock) as Arc<dyn CdpTransport>;
    let session = CdpSession::from_transport("target-1", user_id, "session-1", transport);
    SessionRegistry::instance()
        .open_or_attach(&user_id.to_string(), &run_id.to_string(), || async {
            Ok(session)
        })
        .await
        .unwrap();
    mock
}

fn dom_extractor_payload() -> serde_json::Value {
    // Shape mirrors `dom_extractor.js`'s return value. The snapshot
    // parser assigns ids 1..N in iteration order, so the test fixtures
    // reference [1] = button, [2] = input, [3] = link.
    json!({
        "result": {
            "value": {
                "url": "https://example.com/page",
                "title": "Example",
                "viewport": { "width": 1280, "height": 800, "device_pixel_ratio": 1 },
                "text_excerpt": "Sign in to your account\nForgot password?",
                "elements": [
                    {
                        "tag": "button",
                        "role_hint": null,
                        "label": "Save",
                        "bounds": { "x": 100, "y": 200, "width": 60, "height": 30 },
                        "xpath": "/html/body/button[1]",
                        "disabled": false, "checked": false, "expanded": false,
                        "focused": false, "hidden": false,
                        "attrs": { "type": "submit" }
                    },
                    {
                        "tag": "input",
                        "role_hint": null,
                        "label": "Email",
                        "bounds": { "x": 100, "y": 100, "width": 200, "height": 30 },
                        "xpath": "/html/body/input[1]",
                        "disabled": false, "checked": false, "expanded": false,
                        "focused": false, "hidden": false,
                        "attrs": { "type": "email", "placeholder": "you@example.com", "value": "" }
                    },
                    {
                        "tag": "a",
                        "role_hint": null,
                        "label": "Forgot password?",
                        "bounds": { "x": 100, "y": 300, "width": 120, "height": 20 },
                        "xpath": "/html/body/a[1]",
                        "disabled": false, "checked": false, "expanded": false,
                        "focused": false, "hidden": false,
                        "attrs": { "href": "/forgot" }
                    }
                ]
            }
        }
    })
}

fn nav_history_payload(url: &str) -> serde_json::Value {
    json!({
        "currentIndex": 0,
        "entries": [{ "id": 1, "url": url, "title": "Example" }]
    })
}

// ── browser_observe ────────────────────────────────────────────────

#[tokio::test]
async fn observe_renders_elements_and_emits_payload() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_obs_ok", "r1", mock).await;

    let tool = BrowserObserveTool::new();
    let result = tool
        .execute(json!({ "user_id": "u_obs_ok", "run_id": "r1" }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let md = result.markdown_formatted.expect("markdown payload");
    assert!(md.contains("URL: https://example.com/page"));
    assert!(md.contains("[1] button \"Save\""));
    assert!(md.contains("[2] input \"Email\""));
    assert!(md.contains("[3] link \"Forgot password?\""));
}

#[tokio::test]
async fn observe_compact_tier_caps_elements() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_obs_compact", "r1", mock).await;

    let result = BrowserObserveTool::new()
        .execute(json!({
            "user_id": "u_obs_compact",
            "run_id": "r1",
            "detail": "compact"
        }))
        .await
        .unwrap();
    assert!(!result.is_error);
    // Compact tier skips the text excerpt — assert via its absence.
    let md = result.markdown_formatted.unwrap();
    assert!(!md.contains("--- page text ---"));
}

#[tokio::test]
async fn observe_errors_when_no_session_in_registry() {
    let result = BrowserObserveTool::new()
        .execute(json!({ "user_id": "u_obs_none", "run_id": "r_none" }))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.text().contains("no active session"));
}

#[tokio::test]
async fn observe_errors_on_missing_user_id() {
    let result = BrowserObserveTool::new()
        .execute(json!({ "run_id": "r1" }))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.text().contains("user_id"));
}

// ── browser_act ────────────────────────────────────────────────────

#[tokio::test]
async fn act_navigate_dispatches_page_navigate_and_returns_url() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Page.navigate", json!({ "frameId": "f1" }));
    mock.expect_ok(
        "Page.getNavigationHistory",
        nav_history_payload("https://example.com/landed"),
    );
    // F3-5 chunk 1: post-action preview frame.
    mock.expect_ok("Page.captureScreenshot", json!({ "data": "iVBORw0KGgo=" }));
    install_session("u_act_nav", "r1", mock.clone()).await;

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_act_nav",
            "run_id": "r1",
            "verb": "navigate",
            "url": "https://example.com/landed"
        }))
        .await
        .unwrap();
    assert!(!result.is_error);

    let observed = mock.observed();
    assert_eq!(observed[0].0, "Page.navigate");
    assert_eq!(observed[0].1["url"], "https://example.com/landed");
    assert_eq!(observed[1].0, "Page.getNavigationHistory");
}

#[tokio::test]
async fn act_click_snapshots_then_dispatches_mouse_press_release() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    mock.expect_ok("Input.dispatchMouseEvent", json!({}));
    mock.expect_ok("Input.dispatchMouseEvent", json!({}));
    mock.expect_ok(
        "Page.getNavigationHistory",
        nav_history_payload("https://example.com/page"),
    );
    // F3-5 chunk 1: post-action preview frame.
    mock.expect_ok("Page.captureScreenshot", json!({ "data": "iVBORw0KGgo=" }));
    install_session("u_act_click", "r1", mock.clone()).await;

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_act_click",
            "run_id": "r1",
            "verb": "click",
            "element_id": 1
        }))
        .await
        .unwrap();
    assert!(!result.is_error, "got error: {}", result.text());
    let observed = mock.observed();
    // [Runtime.evaluate, dispatchMouseEvent(press), dispatchMouseEvent(release),
    //  Page.getNavigationHistory, Page.captureScreenshot (F3-5 chunk 1)]
    assert_eq!(observed.len(), 5);
    assert_eq!(observed[1].1["type"], "mousePressed");
    assert_eq!(observed[2].1["type"], "mouseReleased");
    // Click coords should be the element [1] bounds center: x=100..160 -> 130, y=200..230 -> 215.
    assert_eq!(observed[1].1["x"], 130.0);
    assert_eq!(observed[1].1["y"], 215.0);
    assert_eq!(observed[4].0, "Page.captureScreenshot");
}

#[tokio::test]
async fn act_click_errors_when_element_id_missing_from_snapshot() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_act_missing", "r1", mock).await;

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_act_missing",
            "run_id": "r1",
            "verb": "click",
            "element_id": 999
        }))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.text().contains("[999]"));
}

#[tokio::test]
async fn act_type_clicks_focus_then_inserts_text() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    mock.expect_ok("Input.dispatchMouseEvent", json!({})); // click press
    mock.expect_ok("Input.dispatchMouseEvent", json!({})); // click release
                                                           // Default TypeOptions humanized — dispatches one Input.dispatchKeyEvent
                                                           // per char in "hi" = 2 calls.
    mock.expect_ok("Input.dispatchKeyEvent", json!({}));
    mock.expect_ok("Input.dispatchKeyEvent", json!({}));
    mock.expect_ok(
        "Page.getNavigationHistory",
        nav_history_payload("https://example.com/page"),
    );
    // F3-5 chunk 1: post-action preview frame.
    mock.expect_ok("Page.captureScreenshot", json!({ "data": "iVBORw0KGgo=" }));
    install_session("u_act_type", "r1", mock.clone()).await;

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_act_type",
            "run_id": "r1",
            "verb": "type",
            "element_id": 2,
            "text": "hi"
        }))
        .await
        .unwrap();
    assert!(!result.is_error, "got error: {}", result.text());
    let observed = mock.observed();
    // 6 baseline calls + 1 F3-5 chunk 1 captureScreenshot.
    assert_eq!(observed.len(), 7);
    assert_eq!(observed[3].0, "Input.dispatchKeyEvent");
    assert_eq!(observed[3].1["text"], "h");
    assert_eq!(observed[4].1["text"], "i");
    assert_eq!(observed[6].0, "Page.captureScreenshot");
}

#[tokio::test]
async fn act_scroll_dispatches_mouse_wheel() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Input.dispatchMouseEvent", json!({}));
    mock.expect_ok(
        "Page.getNavigationHistory",
        nav_history_payload("https://example.com/page"),
    );
    // F3-5 chunk 1: post-action preview frame.
    mock.expect_ok("Page.captureScreenshot", json!({ "data": "iVBORw0KGgo=" }));
    install_session("u_act_scroll", "r1", mock.clone()).await;

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_act_scroll",
            "run_id": "r1",
            "verb": "scroll",
            "dy": 500
        }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let observed = mock.observed();
    assert_eq!(observed[0].1["type"], "mouseWheel");
    assert_eq!(observed[0].1["deltaY"], 500.0);
}

#[tokio::test]
async fn act_unknown_verb_errors() {
    let mock = Arc::new(MockTransport::new());
    install_session("u_act_unknown", "r1", mock).await;
    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_act_unknown",
            "run_id": "r1",
            "verb": "teleport"
        }))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.text().contains("unknown verb"));
}

#[tokio::test]
async fn act_errors_when_no_session_in_registry() {
    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_act_none",
            "run_id": "r_none",
            "verb": "scroll",
            "dy": 100
        }))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.text().contains("no active session"));
}

// ── browser_extract ────────────────────────────────────────────────

#[tokio::test]
async fn extract_returns_per_id_label_value_href() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_ext_ok", "r1", mock).await;

    let result = BrowserExtractTool::new()
        .execute(json!({
            "user_id": "u_ext_ok",
            "run_id": "r1",
            "element_ids": [2, 3]
        }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let payload = match &result.content[0] {
        crate::openhuman::tools::traits::ToolContent::Json { data } => data.clone(),
        _ => panic!("expected JSON content"),
    };
    let elements = payload["elements"].as_array().unwrap();
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0]["id"], 2);
    assert_eq!(elements[0]["label"], "Email");
    assert_eq!(elements[1]["id"], 3);
    assert_eq!(elements[1]["href"], "/forgot");
}

#[tokio::test]
async fn extract_marks_unknown_ids_with_error_field() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_ext_unknown", "r1", mock).await;

    let result = BrowserExtractTool::new()
        .execute(json!({
            "user_id": "u_ext_unknown",
            "run_id": "r1",
            "element_ids": [999]
        }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let payload = match &result.content[0] {
        crate::openhuman::tools::traits::ToolContent::Json { data } => data.clone(),
        _ => panic!("expected JSON content"),
    };
    assert_eq!(payload["elements"][0]["error"], "not in snapshot");
}

#[tokio::test]
async fn extract_text_pattern_filters_matching_lines() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_ext_pattern", "r1", mock).await;

    let result = BrowserExtractTool::new()
        .execute(json!({
            "user_id": "u_ext_pattern",
            "run_id": "r1",
            "text_pattern": "Forgot"
        }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let payload = match &result.content[0] {
        crate::openhuman::tools::traits::ToolContent::Json { data } => data.clone(),
        _ => panic!("expected JSON content"),
    };
    let matches = payload["text_matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0].as_str().unwrap().contains("Forgot password"));
}

#[tokio::test]
async fn extract_invalid_regex_errors() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_ext_badre", "r1", mock).await;

    let result = BrowserExtractTool::new()
        .execute(json!({
            "user_id": "u_ext_badre",
            "run_id": "r1",
            "text_pattern": "[unbalanced"
        }))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.text().contains("invalid text_pattern regex"));
}

#[tokio::test]
async fn extract_errors_when_no_session_in_registry() {
    let result = BrowserExtractTool::new()
        .execute(json!({
            "user_id": "u_ext_none",
            "run_id": "r_none",
            "element_ids": [1]
        }))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.text().contains("no active session"));
}

// ── tool metadata ──────────────────────────────────────────────────

// ── F3-6 chunk 3: wall-clock cost cap ──────────────────────────────

#[tokio::test]
async fn observe_short_circuits_when_wall_clock_cap_exceeded() {
    let mock = Arc::new(MockTransport::new());
    install_session("u_cap_obs", "cap-r1", mock.clone()).await;
    SessionRegistry::instance().set_meta(
        &"u_cap_obs".into(),
        &"cap-r1".into(),
        crate::openhuman::browser_agent::registry::RunMeta {
            dry_run: false,
            workspace_dir: None,
            wall_clock_cap: Some(crate::openhuman::browser_agent::registry::WallClockCap {
                // Set started_at to 1h ago + max_secs = 1 → already exceeded.
                started_at: std::time::Instant::now() - std::time::Duration::from_secs(3600),
                max_secs: 1,
            }),
        },
    );

    let result = BrowserObserveTool::new()
        .execute(json!({ "user_id": "u_cap_obs", "run_id": "cap-r1" }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let md = result.markdown_formatted.expect("markdown");
    assert!(md.contains("[COST CAP] wall_clock"));
    // Cap must trip BEFORE the DOM extractor runs, so no CDP call observed.
    assert_eq!(
        mock.observed().len(),
        0,
        "cap trip must short-circuit before Runtime.evaluate"
    );
}

#[tokio::test]
async fn act_short_circuits_when_wall_clock_cap_exceeded() {
    let mock = Arc::new(MockTransport::new());
    install_session("u_cap_act", "cap-r2", mock.clone()).await;
    SessionRegistry::instance().set_meta(
        &"u_cap_act".into(),
        &"cap-r2".into(),
        crate::openhuman::browser_agent::registry::RunMeta {
            dry_run: false,
            workspace_dir: None,
            wall_clock_cap: Some(crate::openhuman::browser_agent::registry::WallClockCap {
                started_at: std::time::Instant::now() - std::time::Duration::from_secs(3600),
                max_secs: 1,
            }),
        },
    );

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_cap_act",
            "run_id": "cap-r2",
            "verb": "scroll",
            "dy": 500
        }))
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(result
        .markdown_formatted
        .as_deref()
        .unwrap()
        .contains("[COST CAP] wall_clock"));
    assert_eq!(mock.observed().len(), 0);
}

#[tokio::test]
async fn cap_check_returns_none_when_no_cap_installed() {
    // Default RunMeta has wall_clock_cap = None → no short-circuit.
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_no_cap", "cap-r3", mock.clone()).await;
    // NB: no set_meta — default meta is None for everything.

    let result = BrowserObserveTool::new()
        .execute(json!({ "user_id": "u_no_cap", "run_id": "cap-r3" }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let md = result.markdown_formatted.unwrap();
    // Did NOT short-circuit — normal observe output present.
    assert!(!md.contains("[COST CAP]"));
    assert!(md.contains("[1] button"));
}

#[test]
fn tool_names_match_constants() {
    assert_eq!(
        BrowserObserveTool::new().name(),
        super::TOOL_BROWSER_OBSERVE
    );
    assert_eq!(BrowserActTool::new().name(), super::TOOL_BROWSER_ACT);
    assert_eq!(
        BrowserExtractTool::new().name(),
        super::TOOL_BROWSER_EXTRACT
    );
}

#[test]
fn all_tool_names_lists_three() {
    assert_eq!(super::ALL_TOOL_NAMES.len(), 3);
}

// ── F3-6 chunk 1: dry-run mode ─────────────────────────────────────

#[tokio::test]
async fn act_navigate_dry_run_short_circuits_before_dispatching() {
    // No CDP expectations queued — if dry_run leaks through, the
    // mock transport will error out on the unexpected Page.navigate
    // call. (Page.getNavigationHistory is also unexpected since the
    // dry-run path doesn't read after_url.)
    let mock = Arc::new(MockTransport::new());
    install_session("u_dry_nav", "r1", mock.clone()).await;
    SessionRegistry::instance().set_meta(
        &"u_dry_nav".into(),
        &"r1".into(),
        crate::openhuman::browser_agent::registry::RunMeta {
            dry_run: true,
            wall_clock_cap: None,
            workspace_dir: None,
        },
    );

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_dry_nav",
            "run_id": "r1",
            "verb": "navigate",
            "url": "https://example.com/landed"
        }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let md = result.markdown_formatted.expect("markdown payload");
    assert!(md.contains("[DRY RUN]"));
    assert!(md.contains("navigate to https://example.com/landed"));
    assert_eq!(
        mock.observed().len(),
        0,
        "dry run must not dispatch CDP calls"
    );
}

#[tokio::test]
async fn act_scroll_dry_run_short_circuits() {
    let mock = Arc::new(MockTransport::new());
    install_session("u_dry_scroll", "r1", mock.clone()).await;
    SessionRegistry::instance().set_meta(
        &"u_dry_scroll".into(),
        &"r1".into(),
        crate::openhuman::browser_agent::registry::RunMeta {
            dry_run: true,
            wall_clock_cap: None,
            workspace_dir: None,
        },
    );

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_dry_scroll",
            "run_id": "r1",
            "verb": "scroll",
            "dy": 500
        }))
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(result
        .markdown_formatted
        .as_deref()
        .unwrap()
        .contains("dy=500"));
    assert_eq!(mock.observed().len(), 0);
}

#[tokio::test]
async fn act_click_dry_run_still_snapshots_but_does_not_dispatch_mouse() {
    // The dry-run click path still needs the snapshot so the
    // would_have description names the actual element. So we DO
    // expect the Runtime.evaluate (DOM extractor) call, but NOT the
    // subsequent Input.dispatchMouseEvent pair.
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_dry_click", "r1", mock.clone()).await;
    SessionRegistry::instance().set_meta(
        &"u_dry_click".into(),
        &"r1".into(),
        crate::openhuman::browser_agent::registry::RunMeta {
            dry_run: true,
            wall_clock_cap: None,
            workspace_dir: None,
        },
    );

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_dry_click",
            "run_id": "r1",
            "verb": "click",
            "element_id": 1
        }))
        .await
        .unwrap();
    assert!(!result.is_error, "got error: {}", result.text());
    let md = result.markdown_formatted.unwrap();
    assert!(md.contains("[DRY RUN]"));
    assert!(md.contains("click [1]"));
    assert!(
        md.contains("Save"),
        "dry-run description should name the element"
    );
    let observed = mock.observed();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].0, "Runtime.evaluate");
}

// ── F3-6 chunk 2: audit-log writes ─────────────────────────────────

#[tokio::test]
async fn observe_writes_audit_row_when_workspace_dir_is_set() {
    use crate::openhuman::browser_agent::safety::audit_log;
    use tempfile::TempDir;

    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_audit_obs", "audit-run-1", mock).await;

    let ws = TempDir::new().unwrap();
    let mut cfg = crate::openhuman::config::Config::default();
    cfg.workspace_dir = ws.path().to_path_buf();
    SessionRegistry::instance().set_meta(
        &"u_audit_obs".into(),
        &"audit-run-1".into(),
        crate::openhuman::browser_agent::registry::RunMeta {
            dry_run: false,
            wall_clock_cap: None,
            workspace_dir: Some(ws.path().to_path_buf()),
        },
    );

    let result = BrowserObserveTool::new()
        .execute(json!({ "user_id": "u_audit_obs", "run_id": "audit-run-1" }))
        .await
        .unwrap();
    assert!(!result.is_error);

    let entries = audit_log::list_for_run(&cfg, "audit-run-1").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].tool_name, "browser_observe");
    assert!(entries[0].result_summary.contains("observed 3 elements"));
}

#[tokio::test]
async fn act_dry_run_still_writes_audit_row_with_dry_run_prefix() {
    use crate::openhuman::browser_agent::safety::audit_log;
    use tempfile::TempDir;

    let mock = Arc::new(MockTransport::new());
    install_session("u_audit_dryrun", "audit-run-2", mock).await;

    let ws = TempDir::new().unwrap();
    let mut cfg = crate::openhuman::config::Config::default();
    cfg.workspace_dir = ws.path().to_path_buf();
    SessionRegistry::instance().set_meta(
        &"u_audit_dryrun".into(),
        &"audit-run-2".into(),
        crate::openhuman::browser_agent::registry::RunMeta {
            dry_run: true,
            wall_clock_cap: None,
            workspace_dir: Some(ws.path().to_path_buf()),
        },
    );

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_audit_dryrun",
            "run_id": "audit-run-2",
            "verb": "scroll",
            "dy": 100
        }))
        .await
        .unwrap();
    assert!(!result.is_error);

    let entries = audit_log::list_for_run(&cfg, "audit-run-2").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].tool_name, "browser_act");
    assert!(entries[0].result_summary.starts_with("[dry_run]"));
    assert!(entries[0].result_summary.contains("scroll dy=100"));
}

#[tokio::test]
async fn audit_no_op_when_workspace_dir_is_none() {
    // Default meta has no workspace_dir — emit_audit must skip silently.
    // We re-verify by NOT installing meta and checking the run still
    // works without panicking.
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Runtime.evaluate", dom_extractor_payload());
    install_session("u_no_audit", "no-audit-run", mock).await;
    // NB: no set_meta — default RunMeta has workspace_dir = None.

    let result = BrowserObserveTool::new()
        .execute(json!({ "user_id": "u_no_audit", "run_id": "no-audit-run" }))
        .await
        .unwrap();
    assert!(!result.is_error);
}

#[tokio::test]
async fn act_without_dry_run_flag_dispatches_normally() {
    // Sanity guard: with no meta installed (default RunMeta), the
    // tool dispatches CDP calls as it does today. Prevents a regression
    // where the meta read accidentally defaults to dry_run = true.
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok("Input.dispatchMouseEvent", json!({})); // scroll wheel
    mock.expect_ok(
        "Page.getNavigationHistory",
        nav_history_payload("https://example.com/page"),
    );
    // F3-5 chunk 1: post-action preview frame.
    mock.expect_ok("Page.captureScreenshot", json!({ "data": "iVBORw0KGgo=" }));
    install_session("u_no_dry", "r1", mock.clone()).await;
    // NB: deliberately NOT calling set_meta — exercise the default.

    let result = BrowserActTool::new()
        .execute(json!({
            "user_id": "u_no_dry",
            "run_id": "r1",
            "verb": "scroll",
            "dy": 200
        }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let md = result.markdown_formatted.unwrap();
    assert!(!md.contains("[DRY RUN]"));
    assert_eq!(mock.observed()[0].0, "Input.dispatchMouseEvent");
}
