//! F3-2 unit tests — feed in synthetic JSON payloads from the DOM
//! extractor and assert the resulting PageSnapshot shape + the
//! rendered LLM text. The DOM extractor itself ships as bundled JS;
//! testing it requires a real CEF target (deferred).

use std::sync::Arc;

use serde_json::json;

use super::elements::{ElementRole, ElementState};
use super::render::{to_llm_text, DetailTier};
use super::snapshot::{estimate_tokens, parse_dom_extractor_output, snapshot, SnapshotOptions};
use crate::openhuman::browser_agent::cdp::session::CdpSession;
use crate::openhuman::browser_agent::cdp::transport::{CdpTransport, MockTransport};

fn session(mock: Arc<MockTransport>) -> CdpSession {
    CdpSession::from_transport("t", "u", "s", mock as Arc<dyn CdpTransport>)
}

// ── parse_dom_extractor_output ─────────────────────────────────

#[test]
fn parse_lifts_url_title_viewport_and_elements() {
    let raw = json!({
        "url": "https://example.com/foo",
        "title": "Foo",
        "viewport": { "width": 1280, "height": 800, "device_pixel_ratio": 2 },
        "text_excerpt": "Hello world",
        "elements": [
            {
                "tag": "button",
                "role_hint": null,
                "label": "Save",
                "bounds": { "x": 10, "y": 20, "width": 80, "height": 30 },
                "xpath": "/html/body/button[1]",
                "disabled": false,
                "checked": false,
                "expanded": false,
                "focused": false,
                "hidden": false,
                "attrs": { "type": "submit" }
            },
            {
                "tag": "a",
                "role_hint": null,
                "label": "Home",
                "bounds": { "x": 0, "y": 0, "width": 50, "height": 20 },
                "xpath": "/html/body/a[1]",
                "disabled": false,
                "checked": false,
                "expanded": false,
                "focused": false,
                "hidden": false,
                "attrs": { "href": "/home" }
            }
        ]
    });
    let snap = parse_dom_extractor_output(&raw).unwrap();
    assert_eq!(snap.url, "https://example.com/foo");
    assert_eq!(snap.title, "Foo");
    assert_eq!(snap.viewport.width, 1280.0);
    assert_eq!(snap.viewport.device_pixel_ratio, 2.0);
    assert_eq!(snap.elements.len(), 2);
    assert_eq!(snap.elements[0].id, 1);
    assert!(matches!(snap.elements[0].role, ElementRole::Button));
    assert_eq!(snap.elements[0].label, "Save");
    assert_eq!(snap.elements[1].id, 2);
    assert!(matches!(snap.elements[1].role, ElementRole::Link));
}

#[test]
fn parse_classifies_input_checkbox_radio_distinctly() {
    let raw = json!({
        "url": "u",
        "title": "",
        "viewport": {},
        "text_excerpt": "",
        "elements": [
            { "tag": "input", "role_hint": null, "label": "Email", "bounds": {}, "xpath": "/i[1]",
              "disabled": false, "checked": false, "expanded": false, "focused": false, "hidden": false,
              "attrs": { "type": "email" } },
            { "tag": "input", "role_hint": null, "label": "Subscribe", "bounds": {}, "xpath": "/i[2]",
              "disabled": false, "checked": true, "expanded": false, "focused": false, "hidden": false,
              "attrs": { "type": "checkbox" } },
            { "tag": "input", "role_hint": null, "label": "Yes", "bounds": {}, "xpath": "/i[3]",
              "disabled": false, "checked": false, "expanded": false, "focused": false, "hidden": false,
              "attrs": { "type": "radio" } }
        ]
    });
    let snap = parse_dom_extractor_output(&raw).unwrap();
    assert!(matches!(snap.elements[0].role, ElementRole::Input));
    assert!(matches!(snap.elements[1].role, ElementRole::Checkbox));
    assert!(snap.elements[1].state.checked);
    assert!(matches!(snap.elements[2].role, ElementRole::Radio));
}

#[test]
fn parse_honours_aria_role_hint_over_tag() {
    let raw = json!({
        "url": "u", "title": "", "viewport": {}, "text_excerpt": "",
        "elements": [{
            "tag": "div",
            "role_hint": "button",
            "label": "Custom button",
            "bounds": {},
            "xpath": "/d[1]",
            "disabled": false, "checked": false, "expanded": false, "focused": false, "hidden": false,
            "attrs": {}
        }]
    });
    let snap = parse_dom_extractor_output(&raw).unwrap();
    assert!(matches!(snap.elements[0].role, ElementRole::Button));
}

#[test]
fn parse_classifies_iframe_and_falls_back_to_other_for_unknown_tags() {
    let raw = json!({
        "url": "u", "title": "", "viewport": {}, "text_excerpt": "",
        "elements": [
            { "tag": "iframe", "role_hint": null, "label": "embed", "bounds": {}, "xpath": "/i[1]",
              "disabled": false, "checked": false, "expanded": false, "focused": false, "hidden": false,
              "attrs": { "src": "https://x.io" } },
            { "tag": "details", "role_hint": null, "label": "More", "bounds": {}, "xpath": "/d[1]",
              "disabled": false, "checked": false, "expanded": true, "focused": false, "hidden": false,
              "attrs": {} }
        ]
    });
    let snap = parse_dom_extractor_output(&raw).unwrap();
    assert!(matches!(
        snap.elements[0].role,
        ElementRole::IframePresent { .. }
    ));
    match &snap.elements[1].role {
        ElementRole::Other { tag } => assert_eq!(tag, "details"),
        other => panic!("expected Other, got {other:?}"),
    }
    assert!(snap.elements[1].state.expanded);
}

// ── render ─────────────────────────────────────────────────────

fn snap_with(elements: Vec<serde_json::Value>) -> super::snapshot::PageSnapshot {
    parse_dom_extractor_output(&json!({
        "url": "https://example.com",
        "title": "Example",
        "viewport": { "width": 1024, "height": 768, "device_pixel_ratio": 1 },
        "text_excerpt": "Some page body text. ".repeat(100),
        "elements": elements
    }))
    .unwrap()
}

#[test]
fn render_standard_lists_every_element_with_id_role_label() {
    let snap = snap_with(vec![
        json!({ "tag": "button", "role_hint": null, "label": "Save", "bounds": {}, "xpath": "/b[1]",
                "disabled": false, "checked": false, "expanded": false, "focused": false, "hidden": false,
                "attrs": {} }),
        json!({ "tag": "a", "role_hint": null, "label": "Home", "bounds": {}, "xpath": "/a[1]",
                "disabled": false, "checked": false, "expanded": false, "focused": false, "hidden": false,
                "attrs": { "href": "/home" } }),
    ]);
    let text = to_llm_text(&snap, DetailTier::Standard);
    assert!(text.contains("URL: https://example.com"));
    assert!(text.contains("Title: Example"));
    assert!(text.contains("[1] button \"Save\""));
    assert!(text.contains("[2] link \"Home\" → /home"));
    assert!(text.contains("--- page text ---"));
}

#[test]
fn render_compact_caps_at_30_elements_and_skips_text() {
    let elements: Vec<serde_json::Value> = (0..50)
        .map(|i| {
            json!({
                "tag": "button",
                "role_hint": null,
                "label": format!("Btn {i}"),
                "bounds": {},
                "xpath": format!("/b[{i}]"),
                "disabled": false, "checked": false, "expanded": false, "focused": false, "hidden": false,
                "attrs": {}
            })
        })
        .collect();
    let snap = snap_with(elements);
    let text = to_llm_text(&snap, DetailTier::Compact);
    let line_count = text.lines().filter(|l| l.starts_with('[')).count();
    assert_eq!(line_count, 30, "compact caps at 30");
    assert!(
        !text.contains("--- page text ---"),
        "compact skips text excerpt"
    );
}

#[test]
fn render_verbose_includes_full_attribute_dump_per_element() {
    let snap = snap_with(vec![json!({
        "tag": "input",
        "role_hint": null,
        "label": "Email",
        "bounds": {},
        "xpath": "/i[1]",
        "disabled": false, "checked": false, "expanded": false, "focused": false, "hidden": false,
        "attrs": { "type": "email", "name": "user_email", "placeholder": "you@example.com" }
    })]);
    let text = to_llm_text(&snap, DetailTier::Verbose);
    // Verbose dumps every attribute (except the ones rendered in the
    // primary trail like type / placeholder).
    assert!(text.contains("name=user_email"));
}

#[test]
fn render_includes_disabled_state_suffix() {
    let snap = snap_with(vec![json!({
        "tag": "button",
        "role_hint": null,
        "label": "Submit",
        "bounds": {},
        "xpath": "/b[1]",
        "disabled": true, "checked": false, "expanded": false, "focused": false, "hidden": false,
        "attrs": {}
    })]);
    let text = to_llm_text(&snap, DetailTier::Standard);
    assert!(text.contains("(disabled)"), "got: {text}");
}

// ── token estimate ─────────────────────────────────────────────

#[test]
fn estimate_tokens_uses_chars_div_4_heuristic() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("hello world"), 11 / 4);
    let long = "a".repeat(1000);
    assert_eq!(estimate_tokens(&long), 250);
}

// ── snapshot() integration with MockTransport ──────────────────

#[tokio::test]
async fn snapshot_invokes_runtime_evaluate_and_parses_result() {
    let mock = Arc::new(MockTransport::new());
    // The evaluate result wraps the JS-return JSON under
    // `result.value` — matching what CDP returns.
    mock.expect_ok(
        "Runtime.evaluate",
        json!({
            "result": {
                "value": {
                    "url": "https://test/",
                    "title": "Test",
                    "viewport": { "width": 800, "height": 600, "device_pixel_ratio": 1 },
                    "text_excerpt": "Body text.",
                    "elements": [
                        { "tag": "button", "role_hint": null, "label": "Go", "bounds": {},
                          "xpath": "/b[1]", "disabled": false, "checked": false,
                          "expanded": false, "focused": false, "hidden": false, "attrs": {} }
                    ]
                }
            }
        }),
    );
    let sess = session(mock.clone());
    let snap = snapshot(&sess, SnapshotOptions::default()).await.unwrap();
    assert_eq!(snap.url, "https://test/");
    assert_eq!(snap.elements.len(), 1);
    assert!(snap.snapshot_token_estimate > 0, "token estimate populated");
}

#[tokio::test]
async fn snapshot_propagates_cdp_error_when_evaluate_fails() {
    let mock = Arc::new(MockTransport::new());
    mock.expect_ok(
        "Runtime.evaluate",
        json!({
            "exceptionDetails": { "text": "TypeError: undefined is not a function" }
        }),
    );
    let sess = session(mock.clone());
    let err = snapshot(&sess, SnapshotOptions::default())
        .await
        .unwrap_err();
    assert!(format!("{err}").contains("evaluate"), "got: {err}");
}
