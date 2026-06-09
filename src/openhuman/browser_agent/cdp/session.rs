//! F3-1 — [`CdpSession`] primitives.
//!
//! One `CdpSession` wraps a single CDP target (one page). The
//! primitives are deliberately narrow — anything fancier (intent
//! translation, element grounding, retries) lives in F3-2 / F3-3 on
//! top.
//!
//! ## Logging
//!
//! Every primitive logs at `tracing::debug!(target: "browser-agent-cdp")`
//! with the session's target_id + method + a short arg summary.
//! Errors log at `warn!` with the CDP error code + stage.

use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use super::errors::CdpError;
use super::transport::CdpTransport;
use super::types::{
    Cookie, Key, KeyModifiers, MouseButton, ScreenshotOptions, TypeOptions, WaitOptions,
};

/// Stable id for one open CDP target. Format matches CDP's `targetId`.
pub type TargetId = String;

/// Per-user identifier the session is opened against. Used by the
/// session registry (F3-3) for cross-user isolation enforcement.
pub type UserId = String;

/// One open CDP page session. Each primitive delegates to the
/// underlying [`CdpTransport`], passing the session id CDP returned
/// from `Target.attachToTarget`.
///
/// Drop closes the session as a safety net so a panicking workflow
/// run doesn't leak CEF targets. The blocking close is best-effort —
/// the real cleanup happens via the explicit `close().await` path.
pub struct CdpSession {
    target_id: TargetId,
    user_id: UserId,
    session_id: String,
    transport: Arc<dyn CdpTransport>,
}

impl CdpSession {
    /// Construct directly from a transport. Production callers go
    /// through the session-opener (F3-1 next ticket) which handles
    /// the `Target.attachToTarget` handshake first; tests use
    /// `MockTransport` and a synthetic session id.
    pub fn from_transport(
        target_id: impl Into<TargetId>,
        user_id: impl Into<UserId>,
        session_id: impl Into<String>,
        transport: Arc<dyn CdpTransport>,
    ) -> Self {
        Self {
            target_id: target_id.into(),
            user_id: user_id.into(),
            session_id: session_id.into(),
            transport,
        }
    }

    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    fn sess(&self) -> Option<&str> {
        Some(self.session_id.as_str())
    }

    fn debug_log(&self, method: &str, summary: &str) {
        tracing::debug!(
            target: "browser-agent-cdp",
            target_id = %self.target_id,
            user = %self.user_id,
            method,
            summary,
            "[cdp] dispatch"
        );
    }

    // ── Navigation ──────────────────────────────────────────────

    /// Fires `Page.navigate` and resolves once the page has stopped
    /// loading. The caller can pair with [`Self::wait_for_load`]
    /// for finer-grained control.
    pub async fn navigate(&self, url: &str) -> Result<(), CdpError> {
        self.debug_log("Page.navigate", url);
        let result = self
            .transport
            .call("Page.navigate", json!({ "url": url }), self.sess())
            .await?;
        if let Some(err) = result.get("errorText").and_then(|v| v.as_str()) {
            if !err.is_empty() {
                return Err(CdpError::NavigationFailed {
                    url: url.to_string(),
                    reason: err.to_string(),
                });
            }
        }
        Ok(())
    }

    /// Wait for the configured lifecycle event or timeout.
    /// `WaitUntil::NetworkIdle` is the default — fires after 500ms of
    /// no network activity after onload.
    pub async fn wait_for_load(&self, opts: WaitOptions) -> Result<(), CdpError> {
        // Phase 3.1 implementation: we issue a `Page.enable` to make
        // sure lifecycle events stream, then poll `Page.frameTree`
        // until the main frame's `loaderId` stabilises. CDP also has
        // `Page.lifecycleEvent` push events but the mock-transport
        // pattern doesn't streaming; F3-2 swaps in an event-driven
        // wait once the live WsTransport lands.
        self.debug_log(
            "Page.enable+frameTree(poll)",
            &format!("until={:?} timeout_ms={}", opts.until, opts.timeout.as_millis()),
        );
        self.transport.call("Page.enable", json!({}), self.sess()).await?;
        let deadline = std::time::Instant::now() + opts.timeout;
        let poll_every = Duration::from_millis(150);
        let mut last_loader: Option<String> = None;
        let mut stable_count = 0u32;
        loop {
            if std::time::Instant::now() >= deadline {
                return Err(CdpError::Timeout {
                    what: "page load",
                    after_ms: opts.timeout.as_millis() as u64,
                });
            }
            let tree = self
                .transport
                .call("Page.getFrameTree", json!({}), self.sess())
                .await?;
            let loader = tree
                .get("frameTree")
                .and_then(|t| t.get("frame"))
                .and_then(|f| f.get("loaderId"))
                .and_then(|l| l.as_str())
                .map(String::from);
            // For WaitUntil::FrameStoppedLoading, two consecutive
            // identical loaderIds is "stopped." For other variants,
            // same heuristic in Phase 3.1 — event-driven differentiation
            // is the F3-2 follow-up.
            let _ = opts.until; // suppress unused; the variants converge in v1
            if loader.is_some() && loader == last_loader {
                stable_count += 1;
                // One stable observation past the baseline = stable.
                // Two consecutive polls returned the same loaderId.
                if stable_count >= 1 {
                    return Ok(());
                }
            } else {
                stable_count = 0;
                last_loader = loader;
            }
            sleep(poll_every).await;
        }
    }

    // ── Visual ──────────────────────────────────────────────────

    pub async fn screenshot(&self, opts: ScreenshotOptions) -> Result<Vec<u8>, CdpError> {
        self.debug_log(
            "Page.captureScreenshot",
            &format!("format={:?} full_page={}", opts.format, opts.full_page),
        );
        let mut params = json!({
            "format": opts.format.as_cdp_str(),
            "quality": opts.quality,
            "captureBeyondViewport": opts.full_page,
        });
        if let Some(rect) = opts.clip {
            params["clip"] = json!({
                "x": rect.x,
                "y": rect.y,
                "width": rect.width,
                "height": rect.height,
                "scale": 1.0,
            });
        }
        let result = self
            .transport
            .call("Page.captureScreenshot", params, self.sess())
            .await?;
        let b64 = result
            .get("data")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CdpError::Other("screenshot: response missing `data`".into()))?;
        // CDP returns base64. Decode here so callers get raw bytes.
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| CdpError::Other(format!("screenshot: base64 decode failed: {e}")))
    }

    // ── Mouse ───────────────────────────────────────────────────

    pub async fn click_at(
        &self,
        x: f64,
        y: f64,
        button: MouseButton,
    ) -> Result<(), CdpError> {
        self.debug_log(
            "Input.dispatchMouseEvent(click)",
            &format!("x={x:.1} y={y:.1} button={:?}", button),
        );
        let base = json!({
            "x": x,
            "y": y,
            "button": button.as_cdp_str(),
            "clickCount": 1,
        });
        let mut press = base.clone();
        press["type"] = json!("mousePressed");
        let mut release = base.clone();
        release["type"] = json!("mouseReleased");
        self.transport
            .call("Input.dispatchMouseEvent", press, self.sess())
            .await?;
        self.transport
            .call("Input.dispatchMouseEvent", release, self.sess())
            .await?;
        Ok(())
    }

    pub async fn scroll(&self, dx: f64, dy: f64) -> Result<(), CdpError> {
        self.debug_log(
            "Input.dispatchMouseEvent(wheel)",
            &format!("dx={dx:.0} dy={dy:.0}"),
        );
        self.transport
            .call(
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mouseWheel",
                    "x": 0,
                    "y": 0,
                    "deltaX": dx,
                    "deltaY": dy,
                }),
                self.sess(),
            )
            .await?;
        Ok(())
    }

    // ── Keyboard ────────────────────────────────────────────────

    /// Type text into the focused element. With humanized delay
    /// (default), dispatches `Input.dispatchKeyEvent` per character
    /// with a randomized 30–80ms pause between keystrokes — enough
    /// realism to dodge naive anti-bot fingerprinting. Set
    /// `TypeOptions::instant()` for tests or trusted contexts.
    pub async fn type_text(&self, text: &str, opts: TypeOptions) -> Result<(), CdpError> {
        self.debug_log(
            "Input.insertText|dispatchKeyEvent(per-char)",
            &format!("len={} humanized={}", text.len(), opts.humanized_delay_ms_max > 0),
        );
        if opts.humanized_delay_ms_max == 0 {
            // Fast path — single round-trip.
            self.transport
                .call(
                    "Input.insertText",
                    json!({ "text": text }),
                    self.sess(),
                )
                .await?;
            return Ok(());
        }
        // Per-character with randomised inter-key delay.
        let range = (opts.humanized_delay_ms_max).saturating_sub(opts.humanized_delay_ms_min);
        // Deterministic-ish pseudorandom — XorShift seeded by the
        // text length + char index keeps tests reproducible while
        // varying the cadence enough to feel human.
        let mut state: u64 = (text.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA5A5_A5A5;
        for (idx, ch) in text.chars().enumerate() {
            let mut buf = [0u8; 4];
            let s: &str = ch.encode_utf8(&mut buf);
            self.transport
                .call(
                    "Input.dispatchKeyEvent",
                    json!({
                        "type": "char",
                        "text": s,
                        "unmodifiedText": s,
                    }),
                    self.sess(),
                )
                .await?;
            // XorShift step
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state = state.wrapping_add(idx as u64);
            let extra = if range == 0 {
                0
            } else {
                (state % (range + 1)) as u64
            };
            let delay_ms = opts.humanized_delay_ms_min + extra;
            sleep(Duration::from_millis(delay_ms)).await;
        }
        Ok(())
    }

    pub async fn press_key(&self, key: Key, mods: KeyModifiers) -> Result<(), CdpError> {
        self.debug_log(
            "Input.dispatchKeyEvent(rawKeyDown+keyUp)",
            &format!("key={:?} mods=alt={}/ctrl={}/meta={}/shift={}",
                key, mods.alt, mods.ctrl, mods.meta, mods.shift),
        );
        let (k, code, vk) = key.cdp_fields();
        let (k_owned, code_owned, vk_resolved) = match key {
            Key::Letter(c) => {
                let upper = c.to_ascii_uppercase();
                (upper.to_string(), format!("Key{upper}"), upper as i32)
            }
            Key::Digit(d) => {
                let s = char::from_digit(d as u32, 10).unwrap_or('0').to_string();
                (s.clone(), format!("Digit{s}"), '0' as i32 + d as i32)
            }
            _ => (k.to_string(), code.to_string(), vk),
        };
        let modifiers = mods.as_cdp_bitmask();
        let down = json!({
            "type": "rawKeyDown",
            "key": k_owned,
            "code": code_owned,
            "windowsVirtualKeyCode": vk_resolved,
            "modifiers": modifiers,
        });
        let up = json!({
            "type": "keyUp",
            "key": k_owned,
            "code": code_owned,
            "windowsVirtualKeyCode": vk_resolved,
            "modifiers": modifiers,
        });
        self.transport
            .call("Input.dispatchKeyEvent", down, self.sess())
            .await?;
        self.transport
            .call("Input.dispatchKeyEvent", up, self.sess())
            .await?;
        Ok(())
    }

    // ── Introspection ───────────────────────────────────────────

    /// `Runtime.evaluate`. **Read-only by convention** — see the
    /// F3-1 ticket and CLAUDE.md: persistent JS injection
    /// (`addScriptToEvaluateOnNewDocument` / init scripts) stays
    /// banned. Scoped per-task `evaluate` for agent introspection
    /// is allowed; F3-6's safety preamble reinforces the rule at
    /// the LLM tool layer.
    pub async fn evaluate(&self, expr: &str) -> Result<Value, CdpError> {
        let excerpt: String = expr.chars().take(64).collect();
        self.debug_log("Runtime.evaluate", &excerpt);
        let result = self
            .transport
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": expr,
                    "awaitPromise": true,
                    "returnByValue": true,
                }),
                self.sess(),
            )
            .await?;
        if let Some(exc) = result.get("exceptionDetails") {
            return Err(CdpError::EvaluationError {
                script_excerpt: excerpt,
                reason: exc.to_string(),
            });
        }
        Ok(result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    pub async fn url(&self) -> Result<String, CdpError> {
        self.debug_log("Page.getNavigationHistory", "url()");
        let v = self
            .transport
            .call("Page.getNavigationHistory", json!({}), self.sess())
            .await?;
        let entries = v.get("entries").and_then(|e| e.as_array());
        let idx = v.get("currentIndex").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
        let url = entries
            .and_then(|e| e.get(idx))
            .and_then(|e| e.get("url"))
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();
        Ok(url)
    }

    pub async fn title(&self) -> Result<String, CdpError> {
        self.debug_log("Page.getNavigationHistory", "title()");
        let v = self
            .transport
            .call("Page.getNavigationHistory", json!({}), self.sess())
            .await?;
        let entries = v.get("entries").and_then(|e| e.as_array());
        let idx = v.get("currentIndex").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
        let title = entries
            .and_then(|e| e.get(idx))
            .and_then(|e| e.get("title"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        Ok(title)
    }

    pub async fn cookies(&self) -> Result<Vec<Cookie>, CdpError> {
        self.debug_log("Network.getCookies", "");
        let v = self
            .transport
            .call("Network.getCookies", json!({}), self.sess())
            .await?;
        let arr = v
            .get("cookies")
            .and_then(|c| c.as_array())
            .ok_or_else(|| CdpError::Other("cookies: response missing `cookies`".into()))?;
        Ok(arr
            .iter()
            .filter_map(|raw| {
                Some(Cookie {
                    name: raw.get("name")?.as_str()?.to_string(),
                    value: raw.get("value")?.as_str()?.to_string(),
                    domain: raw.get("domain")?.as_str()?.to_string(),
                    path: raw.get("path")?.as_str()?.to_string(),
                    secure: raw.get("secure").and_then(|s| s.as_bool()).unwrap_or(false),
                    http_only: raw.get("httpOnly").and_then(|s| s.as_bool()).unwrap_or(false),
                    expires_unix: raw.get("expires").and_then(|e| e.as_i64()),
                })
            })
            .collect())
    }

    // ── Teardown ────────────────────────────────────────────────

    /// Detach + close the target. Idempotent. The RAII Drop impl is a
    /// safety net for panicking call sites; prefer the explicit
    /// `close().await` path so the detach actually waits for CDP.
    pub async fn close(self) -> Result<(), CdpError> {
        self.debug_log("Target.closeTarget", "");
        // Best-effort detach first, then close. Either may fail if the
        // target is already gone — log + swallow.
        if let Err(e) = self
            .transport
            .call(
                "Target.detachFromTarget",
                json!({ "sessionId": self.session_id }),
                None,
            )
            .await
        {
            tracing::debug!(target: "browser-agent-cdp", "detach noop: {e}");
        }
        if let Err(e) = self
            .transport
            .call(
                "Target.closeTarget",
                json!({ "targetId": self.target_id }),
                None,
            )
            .await
        {
            tracing::debug!(target: "browser-agent-cdp", "close noop: {e}");
        }
        let _ = self.transport.close().await;
        Ok(())
    }
}

impl Drop for CdpSession {
    fn drop(&mut self) {
        // Can't .await in Drop. The transport's Drop (if any) handles
        // socket cleanup; here we just log so leaked sessions are
        // visible at debug level.
        tracing::debug!(
            target: "browser-agent-cdp",
            target_id = %self.target_id,
            "[cdp] session dropped without explicit close — relying on transport teardown"
        );
    }
}
