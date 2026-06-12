//! F3-5 chunk 2a — bridge `BrowserPreviewFrame` events onto the web
//! channel's socket bus so the frontend `BrowserPreviewPanel` (chunk
//! 2b) can render them in real time.
//!
//! Mirrors the F-16 `ApprovalSurfaceSubscriber` pattern: a single
//! `EventHandler` registered at boot listens for the new domain
//! event, packs it as a `WebChannelEvent` with `event =
//! "browser_preview_frame"`, and publishes via the existing
//! `publish_web_channel_event` channel. The frame payload (png_b64,
//! viewport, timestamp, action_summary, run_id) rides on the
//! `args` field as a JSON object — keeps `WebChannelEvent`'s wire
//! shape stable while still delivering the binary-ish payload.
//!
//! Routing: workflows have no inherent chat context (cron-triggered
//! runs aren't tied to a thread/client), so `thread_id` and
//! `client_id` are blank. The frontend filters by `event` name and
//! routes to the open run-detail surface based on `run_id`.

use std::sync::Arc;
use std::sync::OnceLock;

use async_trait::async_trait;
use serde_json::json;

use crate::core::event_bus::{subscribe_global, EventHandler, SubscriptionHandle};
use crate::core::event_bus::DomainEvent;
use crate::core::socketio::WebChannelEvent;
use crate::openhuman::channels::providers::web::publish_web_channel_event;

static HANDLE: OnceLock<SubscriptionHandle> = OnceLock::new();

/// Register the bridge subscriber. Idempotent — no-op if already
/// registered. Called from the core boot path alongside other socket
/// bridges.
pub fn register() {
    if HANDLE.get().is_some() {
        return;
    }
    match subscribe_global(Arc::new(BrowserPreviewBridge)) {
        Some(handle) => {
            let _ = HANDLE.set(handle);
            tracing::info!(
                target: "browser-agent-preview",
                "[preview-bridge] registered — BrowserPreviewFrame → browser_preview_frame socket event"
            );
        }
        None => {
            tracing::warn!(
                target: "browser-agent-preview",
                "[preview-bridge] event bus not initialized; subscriber not registered"
            );
        }
    }
}

struct BrowserPreviewBridge;

#[async_trait]
impl EventHandler for BrowserPreviewBridge {
    fn name(&self) -> &str {
        "browser_agent::preview::socket_bridge"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["workflow"])
    }

    async fn handle(&self, event: &DomainEvent) {
        if let DomainEvent::BrowserPreviewFrame {
            run_id,
            png_b64,
            viewport_width,
            viewport_height,
            timestamp_ms,
            action_summary,
        } = event
        {
            let args = json!({
                "run_id": run_id,
                "png_b64": png_b64,
                "viewport_width": viewport_width,
                "viewport_height": viewport_height,
                "timestamp_ms": timestamp_ms,
                "action_summary": action_summary,
            });
            publish_web_channel_event(WebChannelEvent {
                event: "browser_preview_frame".to_string(),
                client_id: String::new(),
                thread_id: String::new(),
                request_id: String::new(),
                args: Some(args),
                ..Default::default()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event_bus::{init_global, publish_global};
    use crate::openhuman::channels::providers::web::subscribe_web_channel_events;

    #[tokio::test]
    async fn bridge_forwards_frame_to_web_channel_event_with_args_payload() {
        init_global(64);
        let mut socket_rx = subscribe_web_channel_events();

        // Manually subscribe (don't rely on `register()`'s OnceLock —
        // sibling tests share state, so a leaked one-shot subscription
        // is safer).
        let handle = subscribe_global(Arc::new(BrowserPreviewBridge))
            .expect("event bus initialised by init_global");
        std::mem::forget(handle);

        publish_global(DomainEvent::BrowserPreviewFrame {
            run_id: "bridge-run-1".into(),
            png_b64: "iVBORw0KGgo=".into(),
            viewport_width: 1280,
            viewport_height: 800,
            timestamp_ms: 1_700_000_000_000,
            action_summary: "click [3] Submit".into(),
        });

        // Drain the WebChannelEvent bus and look for our event.
        let mut found = None;
        for _ in 0..32 {
            match tokio::time::timeout(
                std::time::Duration::from_millis(50),
                socket_rx.recv(),
            )
            .await
            {
                Ok(Ok(e)) if e.event == "browser_preview_frame" => {
                    found = Some(e);
                    break;
                }
                Ok(Ok(_)) => continue, // sibling test events; skip
                _ => break,
            }
        }
        let evt = found.expect("expected a browser_preview_frame WebChannelEvent");
        let args = evt.args.expect("args present");
        assert_eq!(args["run_id"], "bridge-run-1");
        assert_eq!(args["png_b64"], "iVBORw0KGgo=");
        assert_eq!(args["viewport_width"], 1280);
        assert_eq!(args["action_summary"], "click [3] Submit");
    }
}
