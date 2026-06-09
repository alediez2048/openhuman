//! F3-1 — shared types for CDP primitives.
//!
//! Kept narrow on purpose: the LLM-facing tool layer (F3-3) translates
//! natural-language actions into these enum variants, so the surface
//! must stay enumerable + serde-friendly.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Which CEF target / profile a new [`super::CdpSession`] attaches to.
///
/// Default for `BrowserActionConfig` (F3-4) is
/// [`BrowserProfile::EphemeralIsolated`] — a fresh per-run profile so
/// a workflow that doesn't explicitly opt into an authenticated
/// session can't accidentally use the user's logged-in GitHub /
/// banking / etc. The validator (F3-4) cross-checks
/// `ReuseAuthenticated` against `allowed_connections`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserProfile {
    /// Attach to the user's existing `webview_account_<provider>`
    /// session. Errors with [`super::CdpError::PermissionDenied`] when
    /// no matching authenticated session is open.
    ReuseAuthenticated { provider: String },
    /// Spawn a fresh CEF target with a temporary profile. Disposed on
    /// session close. **Default** for safety — no inherited cookies.
    EphemeralIsolated,
    /// Reuse or create a named profile that persists across runs.
    /// First run will need the user to log in interactively (or the
    /// agent will hit the safety preamble's "session expired" path);
    /// subsequent runs reuse the saved cookies.
    NamedPersistent { name: String },
}

impl Default for BrowserProfile {
    fn default() -> Self {
        BrowserProfile::EphemeralIsolated
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub fn as_cdp_str(self) -> &'static str {
        match self {
            MouseButton::Left => "left",
            MouseButton::Right => "right",
            MouseButton::Middle => "middle",
        }
    }
}

/// Keyboard keys mapped to CDP's `Input.dispatchKeyEvent` fields.
/// We model only the keys the agent actually needs — letters / digits
/// go through `type_text`, not `press_key`. The variant order matches
/// the order they're documented in the F3-1 ticket.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    Enter,
    Tab,
    Escape,
    Backspace,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    // Letter + digit shortcuts (Cmd+A, Cmd+C, etc.) are dispatched
    // via `press_key(Letter, mods)`. Variant carries the character.
    Letter(char),
    Digit(u8),
}

impl Key {
    /// Returns (key, code, windowsVirtualKeyCode) per CDP.
    /// See https://chromedevtools.github.io/devtools-protocol/tot/Input/
    pub fn cdp_fields(&self) -> (&'static str, &'static str, i32) {
        match self {
            Key::Enter => ("Enter", "Enter", 13),
            Key::Tab => ("Tab", "Tab", 9),
            Key::Escape => ("Escape", "Escape", 27),
            Key::Backspace => ("Backspace", "Backspace", 8),
            Key::ArrowUp => ("ArrowUp", "ArrowUp", 38),
            Key::ArrowDown => ("ArrowDown", "ArrowDown", 40),
            Key::ArrowLeft => ("ArrowLeft", "ArrowLeft", 37),
            Key::ArrowRight => ("ArrowRight", "ArrowRight", 39),
            // Letters / digits get their virtual key code from the
            // character itself — uppercase A is keyCode 65, etc.
            Key::Letter(_) | Key::Digit(_) => ("", "", 0),
        }
    }
}

/// Modifier bitmask passed to `Input.dispatchKeyEvent.modifiers`.
/// CDP uses: 1=alt, 2=ctrl, 4=meta (cmd), 8=shift.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyModifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub meta: bool,
    pub shift: bool,
}

impl KeyModifiers {
    pub fn as_cdp_bitmask(self) -> i32 {
        let mut m = 0;
        if self.alt {
            m |= 1;
        }
        if self.ctrl {
            m |= 2;
        }
        if self.meta {
            m |= 4;
        }
        if self.shift {
            m |= 8;
        }
        m
    }
}

/// Inputs to [`super::CdpSession::wait_for_load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitOptions {
    pub until: WaitUntil,
    pub timeout: Duration,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            until: WaitUntil::NetworkIdle,
            timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitUntil {
    /// `Page.lifecycleEvent` with `name = "DOMContentLoaded"`.
    DomContentLoaded,
    /// `Page.lifecycleEvent` with `name = "networkIdle"` — 500ms of
    /// no network activity after onload.
    NetworkIdle,
    /// `Page.frameStoppedLoading` for the main frame. Sometimes
    /// preferred over networkIdle for SPAs that hold a persistent
    /// connection open.
    FrameStoppedLoading,
}

/// Inputs to [`super::CdpSession::screenshot`].
/// Note: not `Eq` because [`Rect`] carries `f64` (which isn't Eq).
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenshotOptions {
    pub format: ScreenshotFormat,
    /// 0–100 for JPEG; ignored for PNG.
    pub quality: u8,
    pub clip: Option<Rect>,
    pub full_page: bool,
}

impl Default for ScreenshotOptions {
    fn default() -> Self {
        Self {
            format: ScreenshotFormat::Png,
            quality: 80,
            clip: None,
            full_page: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotFormat {
    Png,
    Jpeg,
}

impl ScreenshotFormat {
    pub fn as_cdp_str(self) -> &'static str {
        match self {
            ScreenshotFormat::Png => "png",
            ScreenshotFormat::Jpeg => "jpeg",
        }
    }
}

/// Inputs to [`super::CdpSession::type_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeOptions {
    /// Per-character delay range, in milliseconds. When `min == max ==
    /// 0`, uses CDP's `Input.insertText` (instant — single round-trip).
    /// Default `[30, 80]` for anti-bot mitigation per the F3-1 ticket.
    pub humanized_delay_ms_min: u64,
    pub humanized_delay_ms_max: u64,
}

impl Default for TypeOptions {
    fn default() -> Self {
        Self {
            humanized_delay_ms_min: 30,
            humanized_delay_ms_max: 80,
        }
    }
}

impl TypeOptions {
    /// Instant insert — no per-char delay. Useful for tests + the
    /// rare case where anti-bot fingerprinting isn't a concern.
    pub fn instant() -> Self {
        Self {
            humanized_delay_ms_min: 0,
            humanized_delay_ms_max: 0,
        }
    }
}

/// CSS-pixel rect, top-left origin. Used by `screenshot.clip` +
/// F3-2's `ActionableElement.bounds`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One cookie row returned by `Network.getCookies`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    #[serde(default)]
    pub expires_unix: Option<i64>,
}
