//! F3-1 unit tests — every primitive exercised against
//! [`MockTransport`]. The mock records every CDP call the SUT
//! issues + returns the queued response, so each test asserts on
//! the exact method + params shape.
//!
//! Real-CEF integration tests live as a follow-up (need the running
//! Tauri shell + a fixture HTML page); F3-1's MVP ships the mock
//! coverage so F3-2 / F3-3 can build against a stable surface.

use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

use super::errors::CdpError;
use super::session::CdpSession;
use super::transport::MockTransport;
use super::types::{
    Key, KeyModifiers, MouseButton, ScreenshotFormat, ScreenshotOptions, TypeOptions,
    WaitOptions, WaitUntil,
};

fn fresh_session() -> (Arc<MockTransport>, CdpSession) {
    let transport = Arc::new(MockTransport::new());
    let session = CdpSession::from_transport(
        "target-123",
        "user-abc",
        "session-xyz",
        transport.clone() as Arc<dyn super::transport::CdpTransport>,
    );
    (transport, session)
}

// ── navigate ────────────────────────────────────────────────────

#[tokio::test]
async fn navigate_dispatches_page_navigate_with_url_and_session_id() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Page.navigate", json!({}));
    sess.navigate("https://example.com").await.unwrap();
    let calls = mock.observed();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "Page.navigate");
    assert_eq!(calls[0].1["url"], "https://example.com");
    assert_eq!(calls[0].2.as_deref(), Some("session-xyz"));
}

#[tokio::test]
async fn navigate_surfaces_error_text_as_navigation_failed() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Page.navigate", json!({ "errorText": "ERR_NAME_NOT_RESOLVED" }));
    let err = sess.navigate("https://does-not-exist.invalid").await.unwrap_err();
    match err {
        CdpError::NavigationFailed { url, reason } => {
            assert_eq!(url, "https://does-not-exist.invalid");
            assert_eq!(reason, "ERR_NAME_NOT_RESOLVED");
        }
        other => panic!("expected NavigationFailed, got {other}"),
    }
}

// ── wait_for_load ───────────────────────────────────────────────

#[tokio::test]
async fn wait_for_load_polls_frame_tree_until_loader_stable() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Page.enable", json!({}));
    // Two successive frame-tree polls with the same loaderId → stable.
    mock.expect_ok(
        "Page.getFrameTree",
        json!({ "frameTree": { "frame": { "loaderId": "L1" } } }),
    );
    mock.expect_ok(
        "Page.getFrameTree",
        json!({ "frameTree": { "frame": { "loaderId": "L1" } } }),
    );
    sess.wait_for_load(WaitOptions {
        until: WaitUntil::NetworkIdle,
        timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();
    // Should have made 1 enable + 2 polls = 3 calls.
    assert_eq!(mock.observed().len(), 3);
}

#[tokio::test]
async fn wait_for_load_returns_timeout_when_loader_never_stabilises() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Page.enable", json!({}));
    // Keep returning different loaderIds → never stable. Queue ~12
    // responses so the 1-second deadline fires before the queue empties.
    for i in 0..30 {
        mock.expect_ok(
            "Page.getFrameTree",
            json!({ "frameTree": { "frame": { "loaderId": format!("L{i}") } } }),
        );
    }
    let err = sess
        .wait_for_load(WaitOptions {
            until: WaitUntil::NetworkIdle,
            timeout: Duration::from_millis(300),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, CdpError::Timeout { .. }));
}

// ── screenshot ──────────────────────────────────────────────────

#[tokio::test]
async fn screenshot_dispatches_capture_screenshot_and_base64_decodes() {
    let (mock, sess) = fresh_session();
    // "Hello" base64-encoded = "SGVsbG8=".
    mock.expect_ok("Page.captureScreenshot", json!({ "data": "SGVsbG8=" }));
    let bytes = sess
        .screenshot(ScreenshotOptions {
            format: ScreenshotFormat::Png,
            quality: 80,
            clip: None,
            full_page: false,
        })
        .await
        .unwrap();
    assert_eq!(bytes, b"Hello");
    let calls = mock.observed();
    assert_eq!(calls[0].1["format"], "png");
    assert_eq!(calls[0].1["captureBeyondViewport"], false);
}

#[tokio::test]
async fn screenshot_carries_clip_rect_when_supplied() {
    use super::types::Rect;
    let (mock, sess) = fresh_session();
    mock.expect_ok("Page.captureScreenshot", json!({ "data": "Zm9v" }));
    sess.screenshot(ScreenshotOptions {
        format: ScreenshotFormat::Jpeg,
        quality: 60,
        clip: Some(Rect { x: 10.0, y: 20.0, width: 100.0, height: 200.0 }),
        full_page: true,
    })
    .await
    .unwrap();
    let p = &mock.observed()[0].1;
    assert_eq!(p["format"], "jpeg");
    assert_eq!(p["quality"], 60);
    assert_eq!(p["captureBeyondViewport"], true);
    assert_eq!(p["clip"]["x"], 10.0);
    assert_eq!(p["clip"]["width"], 100.0);
}

// ── click_at ────────────────────────────────────────────────────

#[tokio::test]
async fn click_at_dispatches_pressed_then_released_mouse_events() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Input.dispatchMouseEvent", json!({}));
    mock.expect_ok("Input.dispatchMouseEvent", json!({}));
    sess.click_at(50.0, 75.0, MouseButton::Left).await.unwrap();
    let calls = mock.observed();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1["type"], "mousePressed");
    assert_eq!(calls[0].1["x"], 50.0);
    assert_eq!(calls[0].1["y"], 75.0);
    assert_eq!(calls[0].1["button"], "left");
    assert_eq!(calls[1].1["type"], "mouseReleased");
}

#[tokio::test]
async fn click_at_uses_correct_button_string_for_right_click() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Input.dispatchMouseEvent", json!({}));
    mock.expect_ok("Input.dispatchMouseEvent", json!({}));
    sess.click_at(1.0, 2.0, MouseButton::Right).await.unwrap();
    assert_eq!(mock.observed()[0].1["button"], "right");
}

// ── type_text ───────────────────────────────────────────────────

#[tokio::test]
async fn type_text_with_instant_options_uses_input_insertText() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Input.insertText", json!({}));
    sess.type_text("hello world", TypeOptions::instant())
        .await
        .unwrap();
    let calls = mock.observed();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "Input.insertText");
    assert_eq!(calls[0].1["text"], "hello world");
}

#[tokio::test]
async fn type_text_humanized_dispatches_one_key_event_per_char() {
    let (mock, sess) = fresh_session();
    for _ in 0.."abc".len() {
        mock.expect_ok("Input.dispatchKeyEvent", json!({}));
    }
    // Use very small delays so the test finishes fast.
    sess.type_text(
        "abc",
        TypeOptions {
            humanized_delay_ms_min: 1,
            humanized_delay_ms_max: 2,
        },
    )
    .await
    .unwrap();
    let calls = mock.observed();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].1["text"], "a");
    assert_eq!(calls[1].1["text"], "b");
    assert_eq!(calls[2].1["text"], "c");
    for c in &calls {
        assert_eq!(c.1["type"], "char");
    }
}

// ── press_key ───────────────────────────────────────────────────

#[tokio::test]
async fn press_key_enter_dispatches_rawKeyDown_then_keyUp_with_correct_codes() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Input.dispatchKeyEvent", json!({}));
    mock.expect_ok("Input.dispatchKeyEvent", json!({}));
    sess.press_key(Key::Enter, KeyModifiers::default()).await.unwrap();
    let calls = mock.observed();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1["type"], "rawKeyDown");
    assert_eq!(calls[0].1["key"], "Enter");
    assert_eq!(calls[0].1["code"], "Enter");
    assert_eq!(calls[0].1["windowsVirtualKeyCode"], 13);
    assert_eq!(calls[1].1["type"], "keyUp");
}

#[tokio::test]
async fn press_key_with_cmd_a_carries_meta_modifier_bitmask() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Input.dispatchKeyEvent", json!({}));
    mock.expect_ok("Input.dispatchKeyEvent", json!({}));
    sess.press_key(
        Key::Letter('a'),
        KeyModifiers {
            meta: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let calls = mock.observed();
    // 4 = meta bit per CDP convention.
    assert_eq!(calls[0].1["modifiers"], 4);
    assert_eq!(calls[0].1["key"], "A");
    assert_eq!(calls[0].1["code"], "KeyA");
}

// ── scroll ──────────────────────────────────────────────────────

#[tokio::test]
async fn scroll_dispatches_mouse_wheel_with_deltas() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Input.dispatchMouseEvent", json!({}));
    sess.scroll(0.0, 500.0).await.unwrap();
    let calls = mock.observed();
    assert_eq!(calls[0].1["type"], "mouseWheel");
    assert_eq!(calls[0].1["deltaX"], 0.0);
    assert_eq!(calls[0].1["deltaY"], 500.0);
}

// ── evaluate ────────────────────────────────────────────────────

#[tokio::test]
async fn evaluate_returns_unwrapped_value() {
    let (mock, sess) = fresh_session();
    mock.expect_ok(
        "Runtime.evaluate",
        json!({ "result": { "value": 42 } }),
    );
    let v = sess.evaluate("1 + 41").await.unwrap();
    assert_eq!(v, Value::from(42));
}

#[tokio::test]
async fn evaluate_surfaces_exception_details_as_evaluation_error() {
    let (mock, sess) = fresh_session();
    mock.expect_ok(
        "Runtime.evaluate",
        json!({ "exceptionDetails": { "text": "ReferenceError: foo is not defined" } }),
    );
    let err = sess.evaluate("foo.bar()").await.unwrap_err();
    assert!(matches!(err, CdpError::EvaluationError { .. }));
}

// ── url / title / cookies ───────────────────────────────────────

#[tokio::test]
async fn url_picks_current_entry_from_navigation_history() {
    let (mock, sess) = fresh_session();
    mock.expect_ok(
        "Page.getNavigationHistory",
        json!({
            "currentIndex": 1,
            "entries": [
                { "url": "about:blank", "title": "" },
                { "url": "https://example.com/", "title": "Example" }
            ]
        }),
    );
    assert_eq!(sess.url().await.unwrap(), "https://example.com/");
}

#[tokio::test]
async fn title_picks_current_entry_from_navigation_history() {
    let (mock, sess) = fresh_session();
    mock.expect_ok(
        "Page.getNavigationHistory",
        json!({
            "currentIndex": 0,
            "entries": [{ "url": "https://x", "title": "X" }]
        }),
    );
    assert_eq!(sess.title().await.unwrap(), "X");
}

#[tokio::test]
async fn cookies_parses_each_row_into_typed_struct() {
    let (mock, sess) = fresh_session();
    mock.expect_ok(
        "Network.getCookies",
        json!({
            "cookies": [
                {
                    "name": "session",
                    "value": "abc123",
                    "domain": ".example.com",
                    "path": "/",
                    "secure": true,
                    "httpOnly": true,
                    "expires": 1_700_000_000
                },
                {
                    "name": "tracker",
                    "value": "xyz",
                    "domain": "example.com",
                    "path": "/",
                    "secure": false,
                    "httpOnly": false
                }
            ]
        }),
    );
    let out = sess.cookies().await.unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].name, "session");
    assert_eq!(out[0].secure, true);
    assert_eq!(out[0].http_only, true);
    assert_eq!(out[0].expires_unix, Some(1_700_000_000));
    assert_eq!(out[1].name, "tracker");
    assert_eq!(out[1].expires_unix, None);
}

// ── close ───────────────────────────────────────────────────────

#[tokio::test]
async fn close_detaches_session_and_closes_target_and_transport() {
    let (mock, sess) = fresh_session();
    mock.expect_ok("Target.detachFromTarget", json!({}));
    mock.expect_ok("Target.closeTarget", json!({}));
    sess.close().await.unwrap();
    let calls = mock.observed();
    assert_eq!(calls[0].0, "Target.detachFromTarget");
    assert_eq!(calls[1].0, "Target.closeTarget");
    assert!(mock.is_closed(), "transport close should have been called");
}
