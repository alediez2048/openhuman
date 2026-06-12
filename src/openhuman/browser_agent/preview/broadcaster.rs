//! F3-5 chunk 1 — capture + broadcast one preview frame.

use base64::Engine;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::event_bus::{publish_global, DomainEvent};
use crate::openhuman::browser_agent::cdp::session::CdpSession;
use crate::openhuman::browser_agent::cdp::types::ScreenshotOptions;

/// Knobs the caller passes through. Empty struct today; F3-5 chunk 2
/// will add `format` (JPEG vs PNG), `quality_hint`, `max_dimensions`,
/// etc. Kept as a struct rather than free args so chunk 2 doesn't
/// require touching every call site.
#[derive(Debug, Clone, Default)]
pub struct PreviewCaptureOptions {
    /// Short human-readable label for the action that triggered the
    /// capture (e.g. `"click [3] Submit"`, `"navigate to https://…"`).
    /// Passed through to the broadcast event so the UI's action log
    /// can show what was happening when the frame was captured.
    pub action_summary: String,
}

/// Capture one screenshot from `session`, encode as base64, and
/// publish a [`DomainEvent::BrowserPreviewFrame`]. Best-effort — a
/// failed screenshot logs `warn!` and returns without publishing. The
/// caller (browser_act) should always continue; preview is observability,
/// not correctness.
pub async fn capture_and_broadcast(
    session: &CdpSession,
    run_id: &str,
    opts: PreviewCaptureOptions,
) {
    let png_bytes = match session.screenshot(ScreenshotOptions::default()).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                target: "browser-agent-preview",
                run = %run_id,
                "[preview] screenshot failed (swallowed): {e}"
            );
            return;
        }
    };
    let png_b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    publish_global(DomainEvent::BrowserPreviewFrame {
        run_id: run_id.to_string(),
        png_b64,
        // F3-5 chunk 2 will resize + capture real viewport dims.
        // Today the default Page.captureScreenshot returns the
        // current viewport at the CEF window's natural resolution;
        // we don't read it back to avoid an extra CDP round-trip.
        viewport_width: 0,
        viewport_height: 0,
        timestamp_ms,
        action_summary: opts.action_summary,
    });
    tracing::debug!(
        target: "browser-agent-preview",
        run = %run_id,
        bytes = png_bytes.len(),
        "[preview] frame broadcast"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event_bus::{init_global, subscribe_global, EventHandler};
    use crate::openhuman::browser_agent::cdp::session::CdpSession;
    use crate::openhuman::browser_agent::cdp::transport::{CdpTransport, MockTransport};
    use async_trait::async_trait;
    use base64::Engine as _;
    use parking_lot::Mutex;
    use serde_json::json;
    use std::sync::Arc;

    fn session_with_mock(mock: Arc<MockTransport>) -> CdpSession {
        let transport = mock as Arc<dyn CdpTransport>;
        CdpSession::from_transport("t-1", "u", "s", transport)
    }

    fn one_pixel_png_b64() -> String {
        // 1x1 transparent PNG (smallest valid PNG payload). Encoded
        // verbatim so the mock returns deterministic bytes.
        base64::engine::general_purpose::STANDARD.encode([
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xfa, 0xcf, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xe5, 0x27, 0xde, 0xfc,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ])
    }

    /// Capture every `BrowserPreviewFrame` event into a shared Vec
    /// so tests can assert on them.
    struct CaptureHandler {
        name: String,
        frames: Arc<Mutex<Vec<(String, String, String)>>>,
    }

    #[async_trait]
    impl EventHandler for CaptureHandler {
        fn name(&self) -> &str {
            &self.name
        }
        fn domains(&self) -> Option<&[&str]> {
            Some(&["workflow"])
        }
        async fn handle(&self, event: &DomainEvent) {
            if let DomainEvent::BrowserPreviewFrame {
                run_id,
                action_summary,
                png_b64,
                ..
            } = event
            {
                self.frames
                    .lock()
                    .push((run_id.clone(), action_summary.clone(), png_b64.clone()));
            }
        }
    }

    fn install_capture(name: &str) -> Arc<Mutex<Vec<(String, String, String)>>> {
        init_global(64);
        let frames = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(CaptureHandler {
            name: name.to_string(),
            frames: frames.clone(),
        });
        // Leak the handle so the subscription lives for the rest of
        // the test (Drop would cancel it).
        let handle = subscribe_global(handler).expect("event bus must be initialised");
        std::mem::forget(handle);
        frames
    }

    #[tokio::test]
    async fn capture_publishes_browser_preview_frame_event() {
        let frames = install_capture("preview-test-positive");
        let mock = Arc::new(MockTransport::new());
        mock.expect_ok(
            "Page.captureScreenshot",
            json!({ "data": one_pixel_png_b64() }),
        );
        let session = session_with_mock(mock);

        capture_and_broadcast(
            &session,
            "test-run-1",
            PreviewCaptureOptions {
                action_summary: "click [3] Submit".into(),
            },
        )
        .await;

        // Background dispatch — give the subscriber task a tick to
        // drain.
        for _ in 0..10 {
            tokio::task::yield_now().await;
            if frames
                .lock()
                .iter()
                .any(|(run_id, _, _)| run_id == "test-run-1")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let captured = frames.lock();
        let mine = captured
            .iter()
            .find(|(run_id, _, _)| run_id == "test-run-1")
            .expect("expected a BrowserPreviewFrame for test-run-1");
        assert_eq!(mine.1, "click [3] Submit");
        assert!(!mine.2.is_empty(), "png_b64 payload must be non-empty");
    }

    #[tokio::test]
    async fn capture_screenshot_failure_is_swallowed_without_event() {
        let frames = install_capture("preview-test-negative");
        // No mock expectation queued → screenshot call errors out.
        let mock = Arc::new(MockTransport::new());
        let session = session_with_mock(mock);

        capture_and_broadcast(&session, "test-run-2", PreviewCaptureOptions::default()).await;

        // Give any dispatch a tick; assert no frame was published
        // for test-run-2 specifically (sibling tests may publish
        // events with their own run_ids — filter on ours).
        for _ in 0..5 {
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let captured = frames.lock();
        assert!(
            !captured.iter().any(|(run_id, _, _)| run_id == "test-run-2"),
            "screenshot failure must not publish a frame event"
        );
    }
}
