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

// ── WsTransport — real WebSocket impl (stub for F3-1; F3-2 fills in) ─

/// Real CDP transport backed by a WebSocket connection to
/// `ws://CDP_HOST:CDP_PORT`. Mirrors `app/src-tauri/src/cdp/conn.rs`
/// but lives in the core so the workflow runtime can use it.
///
/// **Phase 3.1 scope:** the F3-1 ticket explicitly ships the trait +
/// mock first; the live WebSocket impl is gated on F3-2's perception
/// layer actually exercising it against live CEF. Until then this
/// stub returns `CdpError::Other("WsTransport not yet implemented")`
/// so a misconfigured workflow surfaces a clear error.
pub struct WsTransport {
    ws_url: String,
    session_id: Option<String>,
}

impl WsTransport {
    pub fn new(ws_url: impl Into<String>, session_id: Option<String>) -> Self {
        Self {
            ws_url: ws_url.into(),
            session_id,
        }
    }

    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
}

#[async_trait]
impl CdpTransport for WsTransport {
    async fn call(
        &self,
        method: &str,
        _params: Value,
        _session_id: Option<&str>,
    ) -> Result<Value, CdpError> {
        Err(CdpError::Other(format!(
            "WsTransport not yet implemented (call={method}, ws_url={}). \
             F3-1 ships the trait + MockTransport; the live WebSocket \
             impl lands in F3-2 alongside the page-perception layer.",
            self.ws_url
        )))
    }

    async fn close(&self) -> Result<(), CdpError> {
        Ok(())
    }
}
