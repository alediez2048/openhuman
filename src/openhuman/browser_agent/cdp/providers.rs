//! F3-4.5 — provider → host map.
//!
//! When a `BrowserAction` node uses `profile = ReuseAuthenticated { provider }`,
//! the session opener needs to find the corresponding already-authenticated
//! page among CEF's open targets. The match is on the page URL's host
//! component, so each provider slug needs a canonical host string.
//!
//! Single source of truth — both the F3-3 validator (whose
//! `ReuseAuthenticated` check enforces a matching `ConnectionRef::Webview`
//! in `allowed_connections`) AND the runtime opener consult this table.

/// Canonical home URL the opener navigates to when no matching tab is
/// already open. CEF persists per-user cookies in its user-data-dir,
/// so creating a fresh tab at this URL loads in the user's
/// already-authenticated state (or bounces to login if cookies
/// expired — caught by the agent's safety preamble as
/// `{status: "session_expired"}`).
///
/// Returns `None` for unknown providers; the opener surfaces that as
/// `PermissionDenied`.
pub fn home_url(provider: &str) -> Option<&'static str> {
    match provider.to_ascii_lowercase().as_str() {
        "linkedin" => Some("https://www.linkedin.com/feed/"),
        "notion" => Some("https://www.notion.so/"),
        "twitter" | "x" => Some("https://x.com/home"),
        "sora" => Some("https://sora.com/"),
        "instagram" => Some("https://www.instagram.com/"),
        "messenger" => Some("https://www.messenger.com/"),
        "discord" => Some("https://discord.com/channels/@me"),
        "slack" => Some("https://app.slack.com/client"),
        "telegram" => Some("https://web.telegram.org/"),
        "whatsapp" => Some("https://web.whatsapp.com/"),
        _ => None,
    }
}

/// Canonical host substring matched against an open page's URL when
/// the profile is `ReuseAuthenticated { provider }`. Returns `None`
/// for unknown providers — the opener surfaces that as
/// `PermissionDenied` rather than silently attaching to the wrong page.
///
/// Substring match on the host is intentional (rather than exact
/// equality) — `linkedin.com` matches both `www.linkedin.com` and
/// `linkedin.com/in/foo`.
pub fn expected_host(provider: &str) -> Option<&'static str> {
    match provider.to_ascii_lowercase().as_str() {
        "linkedin" => Some("linkedin.com"),
        "notion" => Some("notion.so"),
        "twitter" | "x" => Some("x.com"),
        "sora" => Some("sora.com"),
        "instagram" => Some("instagram.com"),
        "messenger" => Some("messenger.com"),
        // Providers that already have rich shell-side scanners and
        // wouldn't typically be the first browser-agent targets, but
        // map them so a workflow that asks for them resolves cleanly.
        "discord" => Some("discord.com"),
        "slack" => Some("slack.com"),
        "telegram" => Some("web.telegram.org"),
        "whatsapp" => Some("web.whatsapp.com"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{expected_host, home_url};

    #[test]
    fn known_providers_map_to_hosts() {
        assert_eq!(expected_host("linkedin"), Some("linkedin.com"));
        assert_eq!(expected_host("notion"), Some("notion.so"));
        assert_eq!(expected_host("x"), Some("x.com"));
        assert_eq!(expected_host("twitter"), Some("x.com"));
    }

    #[test]
    fn unknown_provider_returns_none() {
        assert_eq!(expected_host("nonesuch"), None);
    }

    #[test]
    fn provider_match_is_case_insensitive() {
        assert_eq!(expected_host("LinkedIn"), Some("linkedin.com"));
        assert_eq!(expected_host("NOTION"), Some("notion.so"));
    }

    #[test]
    fn home_url_returns_authenticated_landing_pages() {
        // The opener navigates here when no matching tab exists. Each
        // must be a URL whose host contains the expected_host match
        // string — otherwise the freshly-created tab fails the
        // `url.contains(host)` filter when the agent loop later checks.
        for provider in [
            "linkedin",
            "notion",
            "x",
            "twitter",
            "sora",
            "instagram",
            "messenger",
        ] {
            let host = expected_host(provider).expect(provider);
            let url = home_url(provider).expect(provider);
            assert!(
                url.contains(host),
                "home_url({provider}) = {url} must contain expected_host = {host}"
            );
        }
    }

    #[test]
    fn home_url_unknown_provider_returns_none() {
        assert_eq!(home_url("nonesuch"), None);
    }
}
