//! F3-3 — per-run [`CdpSession`] registry.
//!
//! The browser tools (browser_observe / browser_act / browser_extract)
//! all need to act on the **same** CDP session within a single
//! workflow run. The registry is the per-process cache keyed by
//! `(user_id, run_id)` so:
//!
//! 1. The first browser_* tool call in a run opens a session.
//! 2. Subsequent calls in the same run reuse it.
//! 3. On run terminal status (Succeeded/Failed/Cancelled), F3-4's
//!    executor hook releases the session via [`SessionRegistry::release`].
//!
//! Cross-user isolation lives at session-open time (`open_session_for_user`)
//! in F3-1; the registry just enforces that the `(user_id, run_id)`
//! tuple matches what's stored.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

use super::cdp::session::{CdpSession, UserId};

/// Stable id for one workflow run. Matches `RunId` from
/// `workflows::types` — kept as a String alias here to avoid a
/// crate-level dependency from `browser_agent` on `workflows`.
pub type RunId = String;

/// Per-run safety metadata the tools read at dispatch time. Set by
/// `execute_browser_action` before opening the session; cleared on
/// release. F3-6 chunk 1 shipped `dry_run`; chunk 2 adds
/// `workspace_dir` so the F3-3 tools can find `workflows.db` and
/// write audit-log rows without holding a `Config`.
#[derive(Debug, Clone, Default)]
pub struct RunMeta {
    /// When true, `browser_act` returns `{ status: "dry_run", … }`
    /// instead of dispatching the CDP primitive. Read-only tools
    /// (`browser_observe`, `browser_extract`) are unaffected.
    pub dry_run: bool,

    /// F3-6 chunk 2: workspace directory the executor passes through
    /// so tools can locate `workflows.db` for audit-log writes
    /// without needing a `Config`. `None` in tests / when audit log
    /// is intentionally disabled.
    pub workspace_dir: Option<std::path::PathBuf>,
}

/// Process-global registry. Singleton — `instance()` returns a
/// `&'static SessionRegistry`. Per-(user_id, run_id) entries live
/// until [`Self::release`] is called from the executor's run
/// finaliser (F3-4 wires this).
pub struct SessionRegistry {
    sessions: Mutex<HashMap<(UserId, RunId), Arc<CdpSession>>>,
    /// Parallel map keyed by the same (user_id, run_id) so the F3-3
    /// tools can read per-run safety flags (dry-run today) without
    /// threading them through every tool argument. Independent
    /// lifecycle from `sessions` so a meta-only entry (e.g. dry-run
    /// before the first browser_observe opens the session) is legal.
    meta: Mutex<HashMap<(UserId, RunId), RunMeta>>,
}

impl SessionRegistry {
    /// Singleton accessor — creates on first call, returns the same
    /// instance forever after. Lock contention is per-`(user_id,
    /// run_id)` key in practice since each workflow run uses its own
    /// entry.
    pub fn instance() -> &'static SessionRegistry {
        static REG: OnceLock<SessionRegistry> = OnceLock::new();
        REG.get_or_init(|| SessionRegistry {
            sessions: Mutex::new(HashMap::new()),
            meta: Mutex::new(HashMap::new()),
        })
    }

    /// Install (or replace) the per-run safety metadata for this
    /// `(user_id, run_id)`. Called by `execute_browser_action`
    /// BEFORE opening the session so the first tool call sees the
    /// correct dry-run flag.
    pub fn set_meta(&self, user_id: &UserId, run_id: &RunId, meta: RunMeta) {
        self.meta
            .lock()
            .insert((user_id.clone(), run_id.clone()), meta);
    }

    /// Read the per-run safety metadata. Returns `RunMeta::default()`
    /// when no entry exists — keeps tool dispatch safe in tests / when
    /// the meta wasn't installed (defaults to non-dry-run, which IS
    /// the lower-blast-radius default for the tool's normal mode of
    /// operation: real CDP calls).
    pub fn get_meta(&self, user_id: &UserId, run_id: &RunId) -> RunMeta {
        self.meta
            .lock()
            .get(&(user_id.clone(), run_id.clone()))
            .cloned()
            .unwrap_or_default()
    }

    /// Return the cached session for this run, or insert + return a
    /// freshly-opened one via the supplied opener closure. The
    /// opener is async + may fail; we hold a synchronous Mutex
    /// across the `await` only to look up / insert, never during
    /// `open`.
    pub async fn open_or_attach<F, Fut>(
        &self,
        user_id: &UserId,
        run_id: &RunId,
        open: F,
    ) -> anyhow::Result<Arc<CdpSession>>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<CdpSession>> + Send,
    {
        // Fast path: already cached.
        if let Some(s) = self
            .sessions
            .lock()
            .get(&(user_id.clone(), run_id.clone()))
            .cloned()
        {
            return Ok(s);
        }
        // Slow path: open outside the lock to keep contention bounded.
        let new_session = open().await?;
        let arc = Arc::new(new_session);
        let mut guard = self.sessions.lock();
        // Race: another caller may have opened between our check + this lock.
        // The newer call wins — drop our `new_session` and return theirs.
        let entry = guard
            .entry((user_id.clone(), run_id.clone()))
            .or_insert_with(|| arc.clone())
            .clone();
        Ok(entry)
    }

    /// Look up a cached session without opening. Returns `None` when
    /// no session is registered for this `(user_id, run_id)`. Used by
    /// F3-3 tools that should NOT auto-open (e.g. browser_extract
    /// when called before browser_observe).
    pub fn get(&self, user_id: &UserId, run_id: &RunId) -> Option<Arc<CdpSession>> {
        self.sessions
            .lock()
            .get(&(user_id.clone(), run_id.clone()))
            .cloned()
    }

    /// Release the session for this `(user_id, run_id)`. Best-effort
    /// async close runs in the background so the executor doesn't
    /// block on session teardown when the run finalises. Also clears
    /// any per-run meta installed via [`Self::set_meta`].
    pub fn release(&self, user_id: &UserId, run_id: &RunId) -> Option<Arc<CdpSession>> {
        let key = (user_id.clone(), run_id.clone());
        let removed = self.sessions.lock().remove(&key);
        self.meta.lock().remove(&key);
        if removed.is_some() {
            tracing::debug!(
                target: "browser-agent-registry",
                user = %user_id,
                run = %run_id,
                "[registry] released session"
            );
        }
        removed
    }

    /// Test-only: drain every session (called by `#[cfg(test)]` setup
    /// fns that need a clean slate without colliding with sibling
    /// tests).
    #[cfg(test)]
    pub fn drain_for_tests(&self) {
        self.sessions.lock().clear();
        self.meta.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::browser_agent::cdp::transport::{CdpTransport, MockTransport};

    fn make_session(uid: &str, sid: &str) -> CdpSession {
        let transport = Arc::new(MockTransport::new()) as Arc<dyn CdpTransport>;
        CdpSession::from_transport("t", uid, sid, transport)
    }

    // NOTE: `SessionRegistry` is a process-global singleton, so these
    // tests share state with one another AND with the F3-3 tool tests
    // (which also stash sessions in it). Each test must use a UNIQUE
    // `(user_id, run_id)` tuple and MUST NOT call `drain_for_tests`,
    // since `cargo test` runs them in parallel and a drain in one
    // test will wipe a sibling's entry mid-flight.

    #[tokio::test]
    async fn open_or_attach_caches_after_first_open() {
        let reg = SessionRegistry::instance();
        let user = "registry_test_u_cache".to_string();
        let run = "r1".to_string();
        let s1 = reg
            .open_or_attach(&user, &run, || async {
                Ok(make_session("registry_test_u_cache", "s1"))
            })
            .await
            .unwrap();
        let s2 = reg
            .open_or_attach(&user, &run, || async {
                panic!("opener must not be called when cached")
            })
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&s1, &s2));
    }

    #[tokio::test]
    async fn get_returns_none_when_no_session_open() {
        let reg = SessionRegistry::instance();
        assert!(reg
            .get(&"registry_test_never_inserted".into(), &"r_never".into())
            .is_none());
    }

    #[tokio::test]
    async fn release_removes_entry_so_next_open_creates_fresh() {
        let reg = SessionRegistry::instance();
        let user = "registry_test_u_release".to_string();
        let run = "r1".to_string();
        let _ = reg
            .open_or_attach(&user, &run, || async {
                Ok(make_session("registry_test_u_release", "s1"))
            })
            .await
            .unwrap();
        let removed = reg.release(&user, &run);
        assert!(removed.is_some());
        assert!(reg.get(&user, &run).is_none());
    }
}
