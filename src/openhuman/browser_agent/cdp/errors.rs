//! F3-1 — typed CDP errors.
//!
//! Each primitive returns `Result<T, CdpError>`. The variants are
//! coarse enough that the LLM-facing tool layer (F3-3) can render
//! a useful failure summary without parsing CDP error codes.

use std::fmt;

#[derive(Debug, Clone)]
pub enum CdpError {
    /// `Page.navigate` failed or the navigation didn't complete in
    /// the configured deadline.
    NavigationFailed { url: String, reason: String },
    /// `wait_for_load` deadline elapsed.
    Timeout { what: &'static str, after_ms: u64 },
    /// Target / session no longer attached — the page was closed,
    /// the CEF webview died, etc.
    TargetClosed { detail: String },
    /// `Runtime.evaluate` returned an exception.
    EvaluationError { script_excerpt: String, reason: String },
    /// WebSocket connection dropped while in flight.
    TransportClosed { reason: String },
    /// Cross-user attach attempt — see `WsTransport::open_session_for_user`.
    PermissionDenied { detail: String },
    /// Catch-all for surfaces not worth bucketing — caller surfaces
    /// the string. Keep narrow.
    Other(String),
}

impl fmt::Display for CdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CdpError::NavigationFailed { url, reason } => {
                write!(f, "cdp: navigation to {url} failed: {reason}")
            }
            CdpError::Timeout { what, after_ms } => {
                write!(f, "cdp: timeout waiting for {what} after {after_ms}ms")
            }
            CdpError::TargetClosed { detail } => write!(f, "cdp: target closed: {detail}"),
            CdpError::EvaluationError { script_excerpt, reason } => {
                write!(f, "cdp: evaluate({script_excerpt}…) failed: {reason}")
            }
            CdpError::TransportClosed { reason } => write!(f, "cdp: transport closed: {reason}"),
            CdpError::PermissionDenied { detail } => write!(f, "cdp: permission denied: {detail}"),
            CdpError::Other(s) => write!(f, "cdp: {s}"),
        }
    }
}

impl std::error::Error for CdpError {}
// anyhow's blanket `impl<E: Error> From<E> for anyhow::Error` already
// covers conversion — adding an explicit impl conflicts. Callers use
// `?` and the blanket impl kicks in automatically.
