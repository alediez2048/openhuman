//! F3-1 — CDP transport trait + impls.
//!
//! Every primitive on [`super::CdpSession`] routes its CDP call
//! through a [`CdpTransport`]. Two reasons for the indirection:
//!
//! 1. **Testability** — the [`MockTransport`] records expected
//!    method+param tuples and returns canned responses, letting
//!    every primitive be unit-tested without standing up a real
//!    Chromium target. The F3-1 ticket's "≥15 unit tests across the
//!    primitives" requirement lives entirely on this seam.
//! 2. **Future flexibility** — Phase 3.2 may swap in a Playwright
//!    backend. Same `CdpSession` surface, different transport.
//!    Phase 3.1 doesn't ship the Playwright impl but the
//!    architecture stays ready.
//!
//! The real [`WsTransport`] lands incrementally; F3-1 ships the
//! trait + mock impl + a stub WsTransport that returns
//! `CdpError::Other` until F3-2 actually needs it against live
//! CEF. Marking this scope cut explicitly so the next ticket knows.

use anyhow::Result;
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::VecDeque;

use super::errors::CdpError;

/// Narrow async trait every CDP transport satisfies. Three concrete
/// methods + a close hook — that's the entire seam between the
/// primitives in `session.rs` and whatever WebSocket / mock / future
/// Playwright implementation handles dispatch.
#[async_trait]
pub trait CdpTransport: Send + Sync {
    /// Send a CDP method call and wait for the matching response.
    /// `session_id` is the per-target session returned by
    /// `Target.attachToTarget` — passed through verbatim to CDP.
    async fn call(
        &self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<Value, CdpError>;

    /// Best-effort close. Idempotent.
    async fn close(&self) -> Result<(), CdpError>;
}

// ── MockTransport — for tests ─────────────────────────────────────

/// One queued expectation: the test pushes `(method, params_predicate,
/// response)` tuples in the order the primitive is expected to call
/// them. `params_predicate` is `None` to accept anything.
pub struct ExpectedCall {
    pub method: String,
    pub params_predicate: Option<Box<dyn Fn(&Value) -> bool + Send + Sync>>,
    pub response: Result<Value, CdpError>,
}

impl std::fmt::Debug for ExpectedCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExpectedCall")
            .field("method", &self.method)
            .field("params_predicate", &self.params_predicate.is_some())
            .field("response_ok", &self.response.is_ok())
            .finish()
    }
}

/// Test double. Returns canned responses in FIFO order; records every
/// `call(method, params, session_id)` the SUT made.
pub struct MockTransport {
    queue: Mutex<VecDeque<ExpectedCall>>,
    /// All calls observed so far — accessible via [`Self::observed`].
    observed: Mutex<Vec<(String, Value, Option<String>)>>,
    /// Set on `close()` — tests can assert teardown happened.
    closed: Mutex<bool>,
}

impl Default for MockTransport {
    fn default() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            observed: Mutex::new(Vec::new()),
            closed: Mutex::new(false),
        }
    }
}

impl MockTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a canned response for the next call matching `method`.
    /// `predicate` is optional — when set, the call's params must
    /// satisfy it or the mock returns `CdpError::Other` instead.
    pub fn expect(
        &self,
        method: impl Into<String>,
        predicate: Option<Box<dyn Fn(&Value) -> bool + Send + Sync>>,
        response: Result<Value, CdpError>,
    ) {
        self.queue.lock().push_back(ExpectedCall {
            method: method.into(),
            params_predicate: predicate,
            response,
        });
    }

    /// Convenience: queue an Ok response with the given JSON body.
    pub fn expect_ok(&self, method: impl Into<String>, body: Value) {
        self.expect(method, None, Ok(body));
    }

    pub fn observed(&self) -> Vec<(String, Value, Option<String>)> {
        self.observed.lock().clone()
    }

    pub fn is_closed(&self) -> bool {
        *self.closed.lock()
    }
}

#[async_trait]
impl CdpTransport for MockTransport {
    async fn call(
        &self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<Value, CdpError> {
        self.observed.lock().push((
            method.to_string(),
            params.clone(),
            session_id.map(String::from),
        ));
        let next = self.queue.lock().pop_front().ok_or_else(|| {
            CdpError::Other(format!(
                "mock transport: no expectation queued for `{method}` (params={params})"
            ))
        })?;
        if next.method != method {
            return Err(CdpError::Other(format!(
                "mock transport: expected `{}`, got `{method}`",
                next.method
            )));
        }
        if let Some(pred) = next.params_predicate {
            if !pred(&params) {
                return Err(CdpError::Other(format!(
                    "mock transport: params predicate rejected `{method}` (params={params})"
                )));
            }
        }
        next.response
    }

    async fn close(&self) -> Result<(), CdpError> {
        *self.closed.lock() = true;
        Ok(())
    }
}

// ── WsTransport — real WebSocket impl (F3-4.5) ──────────────────────

use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use super::{CDP_HOST, CDP_PORT};

/// Single round-trip timeout for the setup-phase request/response
/// dispatch. Long enough to cover a cold attach on a sluggish machine;
/// matches the shell-side `app/src-tauri/src/cdp/conn.rs::CALL_TIMEOUT`.
const CALL_TIMEOUT: Duration = Duration::from_secs(35);

/// HTTP timeout for resolving the browser-level WebSocket URL via
/// `GET /json/version`. Short — if CEF isn't up we want to fail fast.
const VERSION_HTTP_TIMEOUT: Duration = Duration::from_secs(5);

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Real CDP transport backed by a WebSocket connection to CEF.
/// Mirrors `app/src-tauri/src/cdp/conn.rs` but lives in the core so
/// the workflow runtime can drive it.
///
/// **Concurrency.** The F3-3 tools (`browser_observe` / `browser_act`
/// / `browser_extract`) all share one `CdpSession` per (user_id,
/// run_id) via the `SessionRegistry`. The agent loop calls them
/// sequentially within a node, so per-transport serialization via
/// `tokio::sync::Mutex` is enough. Cross-run isolation is at the
/// session level — each `BrowserAction` run opens its own WebSocket.
pub struct WsTransport {
    ws_url: String,
    inner: tokio::sync::Mutex<Inner>,
    next_id: AtomicI64,
}

struct Inner {
    /// `None` after `close()` (or a fatal `call` error). Subsequent
    /// calls return `CdpError::TransportClosed` so the agent loop
    /// sees a clear terminal state instead of an opaque hang.
    stream: Option<WsStream>,
}

impl WsTransport {
    /// Open a fresh WebSocket against `ws_url`. Use
    /// [`Self::connect_to_browser`] when you want the auto-discovery
    /// path; this lower-level constructor is for tests + cases where
    /// the URL is already known.
    pub async fn connect(ws_url: impl Into<String>) -> Result<Self, CdpError> {
        let ws_url = ws_url.into();
        let (stream, _resp) = connect_async(&ws_url)
            .await
            .map_err(|e| CdpError::Other(format!("ws connect failed ({ws_url}): {e}")))?;
        Ok(Self {
            ws_url,
            inner: tokio::sync::Mutex::new(Inner {
                stream: Some(stream),
            }),
            next_id: AtomicI64::new(1),
        })
    }

    /// Resolve the browser-level WebSocket URL via
    /// `GET http://127.0.0.1:19222/json/version` and open it. Mirrors
    /// the shell-side `target.rs::browser_ws_url` fallback chain
    /// (`127.0.0.1` then `localhost`) so the same connectivity quirks
    /// that affect the scanners also affect the agent.
    pub async fn connect_to_browser() -> Result<Self, CdpError> {
        let ws_url = resolve_browser_ws_url().await?;
        Self::connect(ws_url).await
    }

    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }
}

#[async_trait]
impl CdpTransport for WsTransport {
    async fn call(
        &self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<Value, CdpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut req = serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        });
        if let Some(sid) = session_id {
            req["sessionId"] = serde_json::Value::String(sid.to_string());
        }
        let body = serde_json::to_string(&req)
            .map_err(|e| CdpError::Other(format!("encode {method} failed: {e}")))?;

        let mut guard = self.inner.lock().await;
        let stream = guard.stream.as_mut().ok_or(CdpError::TransportClosed {
            reason: "ws transport already closed".into(),
        })?;

        stream
            .send(Message::Text(body))
            .await
            .map_err(|e| CdpError::Other(format!("ws send {method}: {e}")))?;

        loop {
            let msg_opt = tokio::time::timeout(CALL_TIMEOUT, stream.next()).await;
            let msg = match msg_opt {
                Err(_) => {
                    return Err(CdpError::Timeout {
                        what: "cdp call",
                        after_ms: CALL_TIMEOUT.as_millis() as u64,
                    });
                }
                Ok(None) => {
                    guard.stream = None;
                    return Err(CdpError::TransportClosed {
                        reason: "ws stream ended".into(),
                    });
                }
                Ok(Some(Err(e))) => {
                    guard.stream = None;
                    return Err(CdpError::Other(format!("ws recv: {e}")));
                }
                Ok(Some(Ok(m))) => m,
            };
            let text = match msg {
                Message::Text(t) => t,
                Message::Close(_) => {
                    guard.stream = None;
                    return Err(CdpError::TransportClosed {
                        reason: "ws stream ended".into(),
                    });
                }
                _ => continue, // ping/pong/binary/frame — skip
            };
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| CdpError::Other(format!("decode response: {e} (body={text})")))?;
            // Skip unrelated events + responses for other ids — the
            // setup-phase dispatch model from the shell-side conn.rs.
            if v.get("id").and_then(|x| x.as_i64()) != Some(id) {
                continue;
            }
            if let Some(err) = v.get("error") {
                return Err(CdpError::Other(format!("cdp error on {method}: {err}")));
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn close(&self) -> Result<(), CdpError> {
        let mut guard = self.inner.lock().await;
        if let Some(mut stream) = guard.stream.take() {
            // Best-effort — the WebSocket may already be torn down.
            let _ = stream.close(None).await;
        }
        Ok(())
    }
}

/// Resolve the browser-level WebSocket URL via `GET /json/version`.
/// Tries `127.0.0.1` then `localhost` so the same connectivity quirks
/// the shell-side scanners hit also surface here. Public so the
/// session opener can reuse it (e.g. to fail-fast before constructing
/// the transport).
pub async fn resolve_browser_ws_url() -> Result<String, CdpError> {
    let client = reqwest::Client::builder()
        .user_agent("openhuman-browser-agent/1.0")
        .timeout(VERSION_HTTP_TIMEOUT)
        .build()
        .map_err(|e| CdpError::Other(format!("reqwest build: {e}")))?;
    let mut last_err: Option<String> = None;
    for host in [CDP_HOST, "localhost"] {
        let url = format!("http://{host}:{CDP_PORT}/json/version");
        match client.get(&url).send().await {
            Ok(resp) => match resp.json::<Value>().await {
                Ok(v) => {
                    if let Some(ws) = v.get("webSocketDebuggerUrl").and_then(|x| x.as_str()) {
                        return Ok(ws.to_string());
                    }
                    last_err = Some(format!("no webSocketDebuggerUrl in {url}"));
                }
                Err(e) => {
                    last_err = Some(format!("parse {url}: {e}"));
                }
            },
            Err(e) => {
                last_err = Some(format!("GET {url}: {e}"));
            }
        }
    }
    Err(CdpError::Other(last_err.unwrap_or_else(|| {
        "failed to resolve CDP websocket URL".into()
    })))
}

#[cfg(test)]
mod ws_transport_tests {
    //! F3-4.5 — `WsTransport` against an in-memory `tokio_tungstenite::accept_async`
    //! server. Each test spins up a one-shot WS endpoint on a random local
    //! port, exercises the request/response loop, and asserts on the
    //! observed wire format. Avoids any dependency on live CEF.
    use super::*;
    use futures_util::{SinkExt as _, StreamExt as _};
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::Message;

    /// Spawn an in-memory CDP-shaped server that echoes one request +
    /// returns `responder(parsed_request)` as the response. Returns the
    /// `ws://...` URL the test connects to.
    async fn spawn_one_shot<F>(responder: F) -> String
    where
        F: Fn(&Value) -> Value + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            while let Some(Ok(Message::Text(req))) = ws.next().await {
                let parsed: Value = serde_json::from_str(&req).unwrap();
                let resp = responder(&parsed);
                let _ = ws.send(Message::Text(resp.to_string())).await;
            }
        });
        format!("ws://{addr}")
    }

    #[tokio::test]
    async fn call_round_trips_id_and_returns_result() {
        let url = spawn_one_shot(|req| {
            let id = req["id"].as_i64().unwrap();
            assert_eq!(req["method"], "Page.enable");
            json!({ "id": id, "result": { "ok": true } })
        })
        .await;
        let t = WsTransport::connect(url).await.unwrap();
        let v = t
            .call("Page.enable", serde_json::json!({}), None)
            .await
            .unwrap();
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn call_propagates_cdp_error_field() {
        let url = spawn_one_shot(|req| {
            let id = req["id"].as_i64().unwrap();
            json!({
                "id": id,
                "error": { "code": -32601, "message": "method not found" }
            })
        })
        .await;
        let t = WsTransport::connect(url).await.unwrap();
        let err = t
            .call("Bogus.method", serde_json::json!({}), None)
            .await
            .unwrap_err();
        match err {
            CdpError::Other(msg) => {
                assert!(msg.contains("cdp error"));
                assert!(msg.contains("method not found"));
            }
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn call_forwards_session_id_when_set() {
        let url = spawn_one_shot(|req| {
            assert_eq!(req["sessionId"], "s-1");
            let id = req["id"].as_i64().unwrap();
            json!({ "id": id, "result": {} })
        })
        .await;
        let t = WsTransport::connect(url).await.unwrap();
        t.call("Page.enable", serde_json::json!({}), Some("s-1"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn close_marks_transport_closed_and_subsequent_calls_error() {
        let url = spawn_one_shot(|req| {
            let id = req["id"].as_i64().unwrap();
            json!({ "id": id, "result": {} })
        })
        .await;
        let t = WsTransport::connect(url).await.unwrap();
        // Drain one round-trip so the server task progresses past accept.
        t.call("Page.enable", serde_json::json!({}), None)
            .await
            .unwrap();
        t.close().await.unwrap();
        let err = t
            .call("Page.enable", serde_json::json!({}), None)
            .await
            .unwrap_err();
        assert!(matches!(err, CdpError::TransportClosed { .. }));
    }

    #[tokio::test]
    async fn concurrent_calls_serialize_via_mutex() {
        use std::sync::Arc;
        let url = spawn_one_shot(|req| {
            let id = req["id"].as_i64().unwrap();
            json!({ "id": id, "result": { "echo": req["method"].clone() } })
        })
        .await;
        let t = Arc::new(WsTransport::connect(url).await.unwrap());
        let a = tokio::spawn({
            let t = t.clone();
            async move { t.call("MethodA", serde_json::json!({}), None).await }
        });
        let b = tokio::spawn({
            let t = t.clone();
            async move { t.call("MethodB", serde_json::json!({}), None).await }
        });
        let (ra, rb) = tokio::try_join!(a, b).unwrap();
        assert_eq!(ra.unwrap()["echo"], "MethodA");
        assert_eq!(rb.unwrap()["echo"], "MethodB");
    }

    #[tokio::test]
    #[ignore = "depends on no process listening on 127.0.0.1:19222 — flakes when the dev app is running locally during cargo test"]
    async fn resolve_browser_ws_url_errors_when_no_server_listening() {
        // The CDP_PORT is 19222; no real CEF in unit-test scope. The
        // resolver tries 127.0.0.1 then localhost; both should fail
        // fast within the 5s timeout. We only assert it errors, not the
        // exact message (varies per OS / DNS config). The `#[ignore]`
        // is for the local-dev case where `pnpm dev:app` IS running
        // CEF on 19222 while you `cargo test` in another terminal —
        // CI runs cleanly without it.
        let err = resolve_browser_ws_url().await.unwrap_err();
        assert!(matches!(err, CdpError::Other(_)));
    }
}
