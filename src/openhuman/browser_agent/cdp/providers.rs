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
    use super::expected_host;

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
}
