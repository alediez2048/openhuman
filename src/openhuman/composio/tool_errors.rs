//! Structured error rendering for `composio_execute` (F-20).
//!
//! Mirrors the F-19 `McpToolErrorKind` pattern. Previously the
//! `ComposioExecuteTool` returned free-form anyhow strings on failure
//! and the orchestrator LLM confabulated explanations (invented HTTP
//! status codes, fabricated OAuth-scope names, told users to update
//! tokens that were already valid). F-20 classifies the underlying
//! string into a stable kind, renders a verbatim-render block with a
//! per-kind actionable suggestion, and teaches the orchestrator prompt
//! to surface that block as-is.
//!
//! The kinds map roughly to `error_mapping::ComposioErrorClass` but
//! carry additional UI-facing distinctions (`InvalidSlugShape` for the
//! "LLM passed a toolkit name as the slug" pre-dispatch validation;
//! `ScopeBlocked` and `NotCurated` for the per-user scope-pref gates
//! that fire before dispatch in `tools.rs`).

use std::sync::OnceLock;

use regex::Regex;

/// Composio action-slug shape — uppercase + underscore-separated, two or
/// more segments. Catches the F-20 hallucination patterns (`composio`,
/// `linkedin`, `lowercase_slug`) without round-tripping to the backend.
///
/// Examples that pass: `GMAIL_SEND_EMAIL`, `SLACK_SEND_MESSAGE`,
/// `LINKEDIN_CREATE_LINKED_IN_POST`, `GOOGLECALENDAR_EVENTS_LIST`.
///
/// Examples that fail: `composio`, `linkedin`, `gmail_send_email`,
/// `GMAIL`, `_GMAIL_SEND`, `GMAIL__SEND`.
pub(crate) fn composio_slug_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Z][A-Z0-9]*(_[A-Z0-9]+)+$").expect("valid regex"))
}

/// Cheap pre-dispatch check the `ComposioExecuteTool::execute` runs on
/// the inbound `tool` argument. The regex catches the LLM hallucination
/// patterns we see in production logs (toolkit names like `composio`,
/// `linkedin`, lowercase slugs, garbage strings) without requiring a
/// backend round-trip.
pub(crate) fn is_valid_composio_slug(tool: &str) -> bool {
    composio_slug_regex().is_match(tool)
}

/// Stable, UI-facing classification for `composio_execute` failures.
/// Surfaced in the rendered `⚠ Composio tool error` block as the
/// `kind: <label>` line — the orchestrator prompt teaches the LLM to
/// surface this verbatim instead of paraphrasing or inventing details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComposioToolErrorKind {
    /// The `tool` argument didn't match the Composio action-slug shape
    /// (e.g. `composio`, `linkedin`, lowercase string). Pre-dispatch
    /// rejection — never hits the backend.
    InvalidSlugShape,
    /// Backend returned `Toolkit "<X>" is not enabled for this entity`
    /// — the slug shape is valid but the toolkit isn't in the user's
    /// allowlist. Distinct from `ActionNotFound` (toolkit exists but
    /// the action slug within it doesn't).
    ToolkitNotEnabled,
    /// Upstream provider (Gmail, Slack, LinkedIn…) returned 401/403,
    /// or Composio said the connection is unauthorized.
    AuthFailed,
    /// Upstream provider or Composio returned 429 / "rate limit".
    RateLimited,
    /// Upstream provider returned a 4xx/5xx that isn't auth/rate-limit
    /// — Composio successfully proxied a real provider failure (bad
    /// arguments at the provider, invalid recipient, etc).
    UpstreamProviderError,
    /// Per-user scope pref rejected this action before dispatch — the
    /// existing `ToolDecision::BlockedByScope` path.
    ScopeBlocked,
    /// Action isn't in the toolkit's curated whitelist — the existing
    /// `ToolDecision::NotCurated` path.
    NotCurated,
    /// Backend said the action slug doesn't exist for that toolkit.
    /// Distinct from `InvalidSlugShape`: shape is valid (e.g.
    /// `LINKEDIN_NONEXISTENT_ACTION`) but the slug isn't a real action.
    ActionNotFound,
    /// Request exceeded the dispatch timeout.
    Timeout,
    /// Catch-all for failure modes the classifier doesn't recognise.
    /// Carries the raw detail through but flags it explicitly so the
    /// orchestrator doesn't guess.
    Unknown,
}

impl ComposioToolErrorKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::InvalidSlugShape => "invalid_slug_shape",
            Self::ToolkitNotEnabled => "toolkit_not_enabled",
            Self::AuthFailed => "auth_failed",
            Self::RateLimited => "rate_limited",
            Self::UpstreamProviderError => "upstream_provider_error",
            Self::ScopeBlocked => "scope_blocked",
            Self::NotCurated => "not_curated",
            Self::ActionNotFound => "action_not_found",
            Self::Timeout => "timeout",
            Self::Unknown => "unknown",
        }
    }

    /// Per-kind actionable next step. The orchestrator surfaces this
    /// verbatim — the four "MCP and Composio tool failures" rules in
    /// `orchestrator/prompt.md` forbid replacement or paraphrase.
    pub(crate) fn suggestion(self) -> &'static str {
        match self {
            Self::InvalidSlugShape => {
                "Call `composio_list_tools(toolkit='<X>')` first to get real action slugs. Slugs always look like `GMAIL_SEND_EMAIL`, `SLACK_SEND_MESSAGE`, `LINKEDIN_CREATE_LINKED_IN_POST` — uppercase + underscores, never a toolkit name like `composio` / `linkedin` / `slack`."
            }
            Self::ToolkitNotEnabled => {
                "The toolkit isn't connected for this user. Direct the user to Settings → Integrations and ask them to connect the toolkit before retrying."
            }
            Self::AuthFailed => {
                "The connected account's token is expired or revoked. Ask the user to reconnect the integration in Settings → Integrations — do NOT guess at scope names."
            }
            Self::RateLimited => {
                "Upstream rate limit hit. Wait at least a minute before retrying; reduce call frequency if this is part of a batch."
            }
            Self::UpstreamProviderError => {
                "The connected provider rejected the request. The detail below contains the provider's reason — surface it to the user verbatim so they can correct the inputs."
            }
            Self::ScopeBlocked => {
                "The user's scope preference blocks this action. The detail explains which scope and how to lift it; surface verbatim."
            }
            Self::NotCurated => {
                "This action isn't in the curated whitelist for the toolkit. Use `composio_list_tools` to see what's available and pick a curated alternative."
            }
            Self::ActionNotFound => {
                "The action slug doesn't exist on this toolkit. Call `composio_list_tools(toolkit='<X>')` and pick a real slug — do NOT retry the same slug."
            }
            Self::Timeout => {
                "The request exceeded the dispatch timeout. Likely an upstream slowdown — retry once; if it recurs, surface to the user instead of retrying tightly."
            }
            Self::Unknown => {
                "Inspect the detail below. Do NOT invent a root cause; surface the raw detail to the user and ask them to share it for triage."
            }
        }
    }
}

/// Classify an opaque error string emitted by the composio dispatch
/// pipeline (`execute_dispatch.rs` → `error_mapping.rs`) or by the
/// backend into a structured kind. Conservative — returns `Unknown`
/// when no pattern fires.
///
/// Patterns covered:
/// - `[composio:error:<class>]` prefixes from `error_mapping.rs`
/// - Backend's `Toolkit "<X>" is not enabled` (the F-20 repro)
/// - HTTP status codes embedded in error strings
/// - Provider-side "action not found" / "no such action" shapes
pub(crate) fn classify_composio_error(err_string: &str) -> ComposioToolErrorKind {
    let lower = err_string.to_lowercase();

    // The `error_mapping.rs` prefix is the most reliable signal — when
    // the dispatch pipeline already classified the error, trust it.
    if lower.contains("[composio:error:rate_limited]") {
        return ComposioToolErrorKind::RateLimited;
    }
    if lower.contains("[composio:error:insufficient_scope]") {
        return ComposioToolErrorKind::AuthFailed;
    }
    if lower.contains("[composio:error:upstream_provider]") {
        return ComposioToolErrorKind::UpstreamProviderError;
    }
    if lower.contains("[composio:error:validation]") {
        return ComposioToolErrorKind::UpstreamProviderError;
    }

    // The F-20 direct-repro string: `Toolkit "linkedin" is not enabled
    // for this entity`. Distinct from `connection error, try to
    // authenticate` (which means the toolkit IS enabled but the OAuth
    // token died).
    if lower.contains("not enabled for this entity") || lower.contains("toolkit not enabled") {
        return ComposioToolErrorKind::ToolkitNotEnabled;
    }

    if lower.contains("connection error, try to authenticate")
        || lower.contains("unauthorized")
        || lower.contains("token revoked")
        || lower.contains("http 401")
        || lower.contains("http 403")
        || lower.contains("(401 ")
        || lower.contains("(403 ")
    {
        return ComposioToolErrorKind::AuthFailed;
    }

    if lower.contains("rate limit")
        || lower.contains("ratelimited")
        || lower.contains("too many requests")
        || lower.contains("http 429")
        || lower.contains("(429 ")
    {
        return ComposioToolErrorKind::RateLimited;
    }

    if lower.contains("action not found")
        || lower.contains("no such action")
        || lower.contains("unknown action")
        || lower.contains("action does not exist")
    {
        return ComposioToolErrorKind::ActionNotFound;
    }

    if lower.contains("timeout") || lower.contains("timed out") || lower.contains("deadline") {
        return ComposioToolErrorKind::Timeout;
    }

    // Generic upstream provider failure — a 4xx/5xx from the connected
    // provider that didn't match any of the more-specific patterns.
    if lower.contains("http 4")
        || lower.contains("http 5")
        || lower.contains("bad gateway")
        || lower.contains("provider error")
        || lower.contains("failed at the connected provider")
    {
        return ComposioToolErrorKind::UpstreamProviderError;
    }

    ComposioToolErrorKind::Unknown
}

/// Render the F-20 stable error format. The orchestrator prompt
/// surfaces this block verbatim — preserving the leading `⚠ Composio
/// tool error` marker and every labeled line — instead of paraphrasing
/// or inventing details.
pub(crate) fn render_composio_tool_error(
    tool: &str,
    kind: ComposioToolErrorKind,
    detail: &str,
) -> String {
    format!(
        "⚠ Composio tool error\ntool: {tool}\nkind: {}\ndetail: {detail}\nsuggestion: {}\n\n[Surface this block verbatim. Do NOT invent additional error details.]",
        kind.label(),
        kind.suggestion()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── slug-shape regex ────────────────────────────────────────────

    #[test]
    fn slug_regex_accepts_real_action_slugs() {
        assert!(is_valid_composio_slug("GMAIL_SEND_EMAIL"));
        assert!(is_valid_composio_slug("SLACK_SEND_MESSAGE"));
        assert!(is_valid_composio_slug("LINKEDIN_CREATE_LINKED_IN_POST"));
        assert!(is_valid_composio_slug("GOOGLECALENDAR_EVENTS_LIST"));
        assert!(is_valid_composio_slug("NOTION_CREATE_PAGE"));
        // Digit segments are allowed (some action slugs include them).
        assert!(is_valid_composio_slug("GMAIL_V2_SEND"));
    }

    #[test]
    fn slug_regex_rejects_toolkit_names() {
        // The F-20 direct-repro: LLM passed the toolkit name as the slug.
        assert!(!is_valid_composio_slug("composio"));
        assert!(!is_valid_composio_slug("linkedin"));
        assert!(!is_valid_composio_slug("slack"));
        assert!(!is_valid_composio_slug("gmail"));
    }

    #[test]
    fn slug_regex_rejects_malformed_shapes() {
        // Lowercase.
        assert!(!is_valid_composio_slug("gmail_send_email"));
        // No underscore separator (single segment).
        assert!(!is_valid_composio_slug("GMAIL"));
        // Leading underscore.
        assert!(!is_valid_composio_slug("_GMAIL_SEND"));
        // Double underscore (empty segment).
        assert!(!is_valid_composio_slug("GMAIL__SEND"));
        // Empty string.
        assert!(!is_valid_composio_slug(""));
        // Starts with digit (must start with uppercase letter).
        assert!(!is_valid_composio_slug("1_GMAIL_SEND"));
        // Trailing underscore.
        assert!(!is_valid_composio_slug("GMAIL_SEND_"));
    }

    // ── classifier ──────────────────────────────────────────────────

    #[test]
    fn classify_recognises_toolkit_not_enabled() {
        assert_eq!(
            classify_composio_error(
                "Backend returned 400: Toolkit \"linkedin\" is not enabled for this entity"
            ),
            ComposioToolErrorKind::ToolkitNotEnabled
        );
    }

    #[test]
    fn classify_recognises_auth_failed() {
        assert_eq!(
            classify_composio_error("Connection error, try to authenticate"),
            ComposioToolErrorKind::AuthFailed
        );
        assert_eq!(
            classify_composio_error("HTTP 401 Unauthorized"),
            ComposioToolErrorKind::AuthFailed
        );
        assert_eq!(
            classify_composio_error("token revoked by upstream"),
            ComposioToolErrorKind::AuthFailed
        );
    }

    #[test]
    fn classify_recognises_rate_limited() {
        assert_eq!(
            classify_composio_error(
                "[composio:error:rate_limited] GMAIL_SEND_EMAIL hit upstream limit"
            ),
            ComposioToolErrorKind::RateLimited
        );
        assert_eq!(
            classify_composio_error("HTTP 429 Too Many Requests"),
            ComposioToolErrorKind::RateLimited
        );
    }

    #[test]
    fn classify_recognises_upstream_provider_error() {
        assert_eq!(
            classify_composio_error("[composio:error:upstream_provider] `GMAIL_SEND_EMAIL` failed at the connected provider: invalid recipient"),
            ComposioToolErrorKind::UpstreamProviderError
        );
        assert_eq!(
            classify_composio_error("HTTP 422 Unprocessable Entity"),
            ComposioToolErrorKind::UpstreamProviderError
        );
        assert_eq!(
            classify_composio_error("[composio:error:validation] Invalid arguments"),
            ComposioToolErrorKind::UpstreamProviderError
        );
    }

    #[test]
    fn classify_recognises_action_not_found() {
        assert_eq!(
            classify_composio_error("action not found for toolkit linkedin"),
            ComposioToolErrorKind::ActionNotFound
        );
        assert_eq!(
            classify_composio_error("Composio: unknown action `LINKEDIN_NONEXISTENT`"),
            ComposioToolErrorKind::ActionNotFound
        );
    }

    #[test]
    fn classify_recognises_timeout() {
        assert_eq!(
            classify_composio_error("request timed out after 30s"),
            ComposioToolErrorKind::Timeout
        );
        assert_eq!(
            classify_composio_error("deadline exceeded"),
            ComposioToolErrorKind::Timeout
        );
    }

    #[test]
    fn classify_unknown_when_no_pattern_matches() {
        assert_eq!(
            classify_composio_error("some completely novel failure mode"),
            ComposioToolErrorKind::Unknown
        );
    }

    // ── renderer ────────────────────────────────────────────────────

    #[test]
    fn render_carries_stable_shape() {
        let rendered = render_composio_tool_error(
            "LINKEDIN_CREATE_LINKED_IN_POST",
            ComposioToolErrorKind::ToolkitNotEnabled,
            "Backend returned 400: Toolkit \"linkedin\" is not enabled for this entity",
        );
        assert!(rendered.starts_with("⚠ Composio tool error\n"));
        assert!(rendered.contains("tool: LINKEDIN_CREATE_LINKED_IN_POST"));
        assert!(rendered.contains("kind: toolkit_not_enabled"));
        assert!(rendered.contains("detail: Backend returned 400: Toolkit \"linkedin\""));
        assert!(rendered.contains("suggestion: The toolkit isn't connected"));
        assert!(rendered.contains("Surface this block verbatim"));
    }

    #[test]
    fn render_unknown_kind_explicitly_marks_it() {
        let rendered = render_composio_tool_error(
            "MYSTERY_ACTION",
            ComposioToolErrorKind::Unknown,
            "weird internal failure mode",
        );
        assert!(rendered.contains("kind: unknown"));
        assert!(rendered.contains("Do NOT invent a root cause"));
    }

    #[test]
    fn render_invalid_slug_shape_carries_the_actionable_suggestion() {
        // The F-20 anti-confabulation hero case. The orchestrator
        // surfaces this verbatim instead of guessing at scope names or
        // making up backend rejection reasons.
        let rendered = render_composio_tool_error(
            "linkedin",
            ComposioToolErrorKind::InvalidSlugShape,
            "received `linkedin`",
        );
        assert!(rendered.contains("kind: invalid_slug_shape"));
        assert!(rendered.contains("tool: linkedin"));
        assert!(rendered.contains("suggestion: Call `composio_list_tools"));
        assert!(rendered.contains("LINKEDIN_CREATE_LINKED_IN_POST"));
    }
}
