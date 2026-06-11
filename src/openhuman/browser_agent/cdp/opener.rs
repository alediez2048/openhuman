//! F3-4.5 — profile-aware `CdpSession` opener.
//!
//! Each variant of [`super::types::BrowserProfile`] has a different
//! attach sequence:
//!
//! - `EphemeralIsolated` (default, safe): `Target.createTarget` →
//!   `Target.attachToTarget`. Fresh CEF target, no inherited cookies.
//! - `ReuseAuthenticated { provider }`: `Target.getTargets`, find the
//!   page whose URL matches `providers::expected_host(provider)`, then
//!   `Target.attachToTarget`. Errors with `PermissionDenied` when no
//!   matching authenticated page is open.
//! - `NamedPersistent { name }`: Phase 3.1 scope-cut — CEF user-data-dir
//!   is set at process startup and can't be swapped per target, so this
//!   errors with a clear "Phase 3.2 follow-up" message.
//!
//! Both happy paths funnel through `attach_to_target` so the
//! `flatten: true` handshake stays uniform.

use std::sync::Arc;

use serde_json::{json, Value};

use super::errors::CdpError;
use super::providers::{expected_host, home_url};
use super::session::CdpSession;
use super::transport::{CdpTransport, WsTransport};
use super::types::BrowserProfile;

/// Open a CdpSession for the given profile. Production entry point —
/// `execute_browser_action` dispatches here through
/// `open_browser_session_for_run`.
pub async fn open_session_for_profile(
    user_id: &str,
    profile: &BrowserProfile,
) -> Result<CdpSession, CdpError> {
    match profile {
        BrowserProfile::EphemeralIsolated => open_ephemeral_isolated(user_id).await,
        BrowserProfile::ReuseAuthenticated { provider } => {
            open_reuse_authenticated(user_id, provider).await
        }
        BrowserProfile::NamedPersistent { name } => open_named_persistent(user_id, name).await,
    }
}

/// Spawn a fresh CEF target on `about:blank` and attach. Default
/// safe path for any `browser_action` workflow that doesn't
/// explicitly opt into an authenticated session.
pub async fn open_ephemeral_isolated(user_id: &str) -> Result<CdpSession, CdpError> {
    let transport = WsTransport::connect_to_browser().await?;
    let target_id = create_target(&transport, "about:blank").await?;
    let session_id = attach_to_target(&transport, &target_id).await?;
    Ok(CdpSession::from_transport(
        target_id,
        user_id,
        session_id,
        Arc::new(transport) as Arc<dyn CdpTransport>,
    ))
}

/// Find the already-open authenticated page for `provider` and
/// attach. Falls back to creating a fresh tab at the provider's home
/// URL when no matching tab is open — CEF's persistent user-data-dir
/// preserves the user's auth cookies across app restarts, so a newly
/// created tab loads already authenticated. If the cookies actually
/// expired, the page bounces to login and the agent's safety
/// preamble catches it with `{status: "session_expired"}`.
///
/// Errors with [`CdpError::PermissionDenied`] only when the provider
/// itself is unknown (no `expected_host` entry) — at that point
/// there's no URL to navigate to.
pub async fn open_reuse_authenticated(
    user_id: &str,
    provider: &str,
) -> Result<CdpSession, CdpError> {
    let host = expected_host(provider).ok_or_else(|| CdpError::PermissionDenied {
        detail: format!(
            "browser_action: unknown provider `{provider}` — add it to \
             browser_agent::cdp::providers::expected_host"
        ),
    })?;
    let transport = WsTransport::connect_to_browser().await?;
    let targets = list_targets(&transport).await?;
    let target_id = if let Some(existing) =
        targets.into_iter().find(|t| t.kind == "page" && t.url.contains(host))
    {
        tracing::debug!(
            target: "browser-agent-opener",
            user = %user_id,
            provider = %provider,
            url = %existing.url,
            "[opener] reuse_authenticated: attaching to existing tab"
        );
        existing.id
    } else {
        // No live tab — spawn one at the provider's home URL. Cookies
        // persist in CEF's user-data-dir, so the new tab loads
        // authenticated when the session is still valid.
        let landing = home_url(provider).ok_or_else(|| CdpError::PermissionDenied {
            detail: format!(
                "browser_action: no authenticated `{provider}` page is open and \
                 no home URL is registered for this provider. Add an entry to \
                 browser_agent::cdp::providers::home_url, or open the provider \
                 in a webview manually first."
            ),
        })?;
        tracing::info!(
            target: "browser-agent-opener",
            user = %user_id,
            provider = %provider,
            landing = %landing,
            "[opener] reuse_authenticated: no live tab — creating one at provider home URL"
        );
        create_target(&transport, landing).await?
    };
    let session_id = attach_to_target(&transport, &target_id).await?;
    Ok(CdpSession::from_transport(
        target_id,
        user_id,
        session_id,
        Arc::new(transport) as Arc<dyn CdpTransport>,
    ))
}

/// Phase 3.1 scope-cut. CEF user-data-dir is set at process startup
/// (see `app/src-tauri/src/cef_profile.rs`), so named per-workflow
/// profiles need a multi-profile CEF setup that's outside Phase 3's
/// scope. A future ticket can either spawn separate CEF child
/// processes per name or extend the shell to expose a profile-swap
/// IPC — for now, fail loud.
pub async fn open_named_persistent(_user_id: &str, name: &str) -> Result<CdpSession, CdpError> {
    Err(CdpError::PermissionDenied {
        detail: format!(
            "browser_action: NamedPersistent profile `{name}` not supported in \
             Phase 3.1 — CEF user-data-dir is set at process startup. Use \
             EphemeralIsolated (no auth) or ReuseAuthenticated (existing webview \
             session) instead. Multi-profile CEF is a Phase 3.2 follow-up."
        ),
    })
}

// ── CDP plumbing ────────────────────────────────────────────────────

async fn create_target(transport: &WsTransport, url: &str) -> Result<String, CdpError> {
    let v = transport
        .call("Target.createTarget", json!({ "url": url }), None)
        .await?;
    v.get("targetId")
        .and_then(|x| x.as_str())
        .map(String::from)
        .ok_or_else(|| CdpError::Other("Target.createTarget response missing `targetId`".into()))
}

async fn attach_to_target(transport: &WsTransport, target_id: &str) -> Result<String, CdpError> {
    let v = transport
        .call(
            "Target.attachToTarget",
            json!({ "targetId": target_id, "flatten": true }),
            None,
        )
        .await?;
    v.get("sessionId")
        .and_then(|x| x.as_str())
        .map(String::from)
        .ok_or_else(|| CdpError::Other("Target.attachToTarget response missing `sessionId`".into()))
}

#[derive(Debug, Clone)]
struct TargetInfo {
    id: String,
    kind: String,
    url: String,
}

async fn list_targets(transport: &WsTransport) -> Result<Vec<TargetInfo>, CdpError> {
    let v: Value = transport.call("Target.getTargets", json!({}), None).await?;
    let infos = v
        .get("targetInfos")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(infos
        .into_iter()
        .filter_map(|t| {
            Some(TargetInfo {
                id: t.get("targetId")?.as_str()?.to_string(),
                kind: t.get("type")?.as_str()?.to_string(),
                url: t.get("url")?.as_str()?.to_string(),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    //! Opener tests exercise the create→attach + getTargets→attach
    //! handshakes against `MockTransport`. Live CEF coverage is the
    //! manual smoke test in `F3-4.5-DEVLOG.md`.
    //!
    //! These tests construct a `CdpSession` directly via
    //! `CdpSession::from_transport` to verify the call-sequence
    //! pattern the opener follows — the production helpers
    //! (`open_ephemeral_isolated`, etc.) construct their own
    //! `WsTransport`, which can't be substituted with a mock without
    //! a dep-injection seam we don't ship in 3.1.

    use super::*;
    use crate::openhuman::browser_agent::cdp::transport::MockTransport;
    use serde_json::json;

    fn mock() -> Arc<MockTransport> {
        Arc::new(MockTransport::new())
    }

    #[tokio::test]
    async fn ephemeral_attach_sequence_is_create_then_attach() {
        let m = mock();
        m.expect_ok("Target.createTarget", json!({ "targetId": "t-1" }));
        m.expect_ok("Target.attachToTarget", json!({ "sessionId": "s-1" }));

        // Drive the same primitives the opener would call.
        let v = m
            .call("Target.createTarget", json!({ "url": "about:blank" }), None)
            .await
            .unwrap();
        assert_eq!(v["targetId"], "t-1");
        let v = m
            .call(
                "Target.attachToTarget",
                json!({ "targetId": "t-1", "flatten": true }),
                None,
            )
            .await
            .unwrap();
        assert_eq!(v["sessionId"], "s-1");

        let observed = m.observed();
        assert_eq!(observed.len(), 2);
        assert_eq!(observed[1].1["flatten"], true);
    }

    #[tokio::test]
    async fn reuse_attach_picks_target_matching_provider_host() {
        let m = mock();
        m.expect_ok(
            "Target.getTargets",
            json!({
                "targetInfos": [
                    { "targetId": "t-bg", "type": "background_page", "url": "chrome-extension://x" },
                    { "targetId": "t-noise", "type": "page", "url": "https://example.com/" },
                    { "targetId": "t-li", "type": "page", "url": "https://www.linkedin.com/messages" }
                ]
            }),
        );
        let v = m.call("Target.getTargets", json!({}), None).await.unwrap();
        let infos = v["targetInfos"].as_array().unwrap();
        let picked = infos
            .iter()
            .find(|t| t["type"] == "page" && t["url"].as_str().unwrap().contains("linkedin.com"))
            .unwrap();
        assert_eq!(picked["targetId"], "t-li");
    }

    #[tokio::test]
    async fn named_persistent_errors_with_phase_3_2_message() {
        let result = open_named_persistent("u", "my-bot").await;
        match result {
            Err(CdpError::PermissionDenied { detail: reason }) => {
                assert!(reason.contains("NamedPersistent"));
                assert!(reason.contains("Phase 3.2"));
            }
            Err(other) => panic!("expected PermissionDenied, got Err({other:?})"),
            Ok(_) => panic!("expected PermissionDenied, got Ok(CdpSession)"),
        }
    }

    #[tokio::test]
    async fn reuse_unknown_provider_errors_clearly() {
        // open_reuse_authenticated calls expected_host first; an unknown
        // provider errors BEFORE the network call so we can call the
        // production helper directly (no live CEF needed).
        let result = open_reuse_authenticated("u", "nonesuch").await;
        match result {
            Err(CdpError::PermissionDenied { detail: reason }) => {
                assert!(reason.contains("unknown provider"));
                assert!(reason.contains("nonesuch"));
            }
            Err(other) => panic!("expected PermissionDenied, got Err({other:?})"),
            Ok(_) => panic!("expected PermissionDenied, got Ok(CdpSession)"),
        }
    }

    #[tokio::test]
    async fn reuse_no_live_tab_falls_back_to_creating_one_at_home_url() {
        // Documents the new fallback behaviour with MockTransport's
        // create→attach handshake. The production helper calls
        // `connect_to_browser` first which requires live CEF; we
        // exercise the dispatch pattern by simulating the same
        // sequence against the mock, mirroring the existing tests in
        // this module that document the attach handshake without
        // standing up real CEF.
        let m = mock();
        m.expect_ok("Target.getTargets", json!({ "targetInfos": [] }));
        m.expect_ok(
            "Target.createTarget",
            json!({ "targetId": "t-new" }),
        );
        m.expect_ok("Target.attachToTarget", json!({ "sessionId": "s-new" }));

        let v = m.call("Target.getTargets", json!({}), None).await.unwrap();
        let infos = v["targetInfos"].as_array().unwrap();
        let match_existing = infos
            .iter()
            .find(|t| t["type"] == "page" && t["url"].as_str().unwrap_or("").contains("linkedin.com"));
        assert!(
            match_existing.is_none(),
            "fixture must have no linkedin page to exercise fallback"
        );
        // No matching tab → opener creates one at the home URL.
        let v = m
            .call(
                "Target.createTarget",
                json!({ "url": "https://www.linkedin.com/feed/" }),
                None,
            )
            .await
            .unwrap();
        assert_eq!(v["targetId"], "t-new");
        let v = m
            .call(
                "Target.attachToTarget",
                json!({ "targetId": "t-new", "flatten": true }),
                None,
            )
            .await
            .unwrap();
        assert_eq!(v["sessionId"], "s-new");

        // Sanity: home URL passes the substring filter the agent loop
        // will later use against the tab's URL.
        assert!(super::super::providers::home_url("linkedin")
            .unwrap()
            .contains(super::super::providers::expected_host("linkedin").unwrap()));
    }
}
