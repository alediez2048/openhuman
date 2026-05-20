//! Operational core for webview login detection.
//!
//! See the parent `mod.rs` for the why/how. This file owns the actual
//! cookie-store probe.

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::PathBuf;

/// Env var set by the Tauri shell to the shared CEF cookies SQLite
/// path. See `app/src-tauri/src/lib.rs`.
pub(crate) const COOKIES_DB_ENV: &str = "OPENHUMAN_CEF_COOKIES_DB";

/// A provider we surface in the welcome snapshot.
///
/// Two probe paths, OR'd together — a provider is "logged in" if **either**
/// fires:
///
/// 1. Cookie probe — `host_suffix` is matched against Chromium's `host_key`
///    column with a trailing-wildcard SQL `LIKE`; `session_cookie_names`
///    are the cookie `name` values that indicate an active login. Used by
///    Slack / Discord / LinkedIn / WhatsApp Web / X / Instagram / Messenger.
///
/// 2. IndexedDB probe — `indexeddb_origin` names the Chromium
///    `IndexedDB/<origin>_<port>.indexeddb.leveldb/` directory that the
///    provider populates on sign-in. Used by Telegram Web (which stores
///    its auth blob entirely in IndexedDB, no cookies). When `None`, this
///    path is skipped.
struct Provider {
    /// Stable key surfaced in the JSON snapshot (e.g. `"gmail"`).
    key: &'static str,
    /// Host suffix the auth cookie must live under. Chromium stores
    /// host_key with a leading dot for domain cookies (e.g.
    /// `.google.com`) or the full host for host-only cookies. We match
    /// with `%suffix`.
    host_suffix: &'static str,
    /// Cookie names that indicate a logged-in session. Picked per-provider
    /// to avoid false positives from analytics/consent cookies.
    session_cookie_names: &'static [&'static str],
    /// Optional IndexedDB origin directory prefix to probe as a fallback
    /// (e.g. `"https_web.telegram.org_0"`). When present, the absence of a
    /// matching cookie is no longer authoritative — the IndexedDB folder
    /// must also be empty before we report `logged_in: false`.
    indexeddb_origin: Option<&'static str>,
}

/// Providers the welcome agent cares about. Keep this list aligned
/// with the webview accounts system in `app/src-tauri/src/webview_accounts/`.
///
/// Curated for the Connections Hub Browser Accounts surface: providers whose
/// web client is usable inside an embedded Chromium *and* where the agent
/// has unique value vs the official API. Gmail / Google Messages / Zoom
/// were removed because they either fight CEF (Google's anti-automation
/// stack) or force native-app redirects (Zoom).
pub(crate) const PROVIDERS: &[Provider] = &[
    Provider {
        key: "whatsapp",
        host_suffix: "web.whatsapp.com",
        session_cookie_names: &["wa_ul", "wa_build"],
        indexeddb_origin: None,
    },
    Provider {
        // Telegram Web stores its auth keys (`dc1_auth_key`, etc.) in
        // IndexedDB rather than cookies. Cookies remain in the probe so a
        // future Telegram change that adds a session cookie still fires;
        // the IndexedDB path is the primary signal today.
        key: "telegram",
        host_suffix: "web.telegram.org",
        session_cookie_names: &["stel_ssid", "stel_token"],
        indexeddb_origin: Some("https_web.telegram.org_0"),
    },
    Provider {
        key: "slack",
        host_suffix: ".slack.com",
        session_cookie_names: &["d", "d-s"],
        indexeddb_origin: None,
    },
    Provider {
        key: "discord",
        host_suffix: ".discord.com",
        session_cookie_names: &["__Secure-recent_session", "__Secure-authjs.session-token"],
        indexeddb_origin: None,
    },
    Provider {
        key: "linkedin",
        host_suffix: ".linkedin.com",
        session_cookie_names: &["li_at"],
        indexeddb_origin: None,
    },
    Provider {
        key: "twitter",
        host_suffix: ".x.com",
        session_cookie_names: &["auth_token"],
        indexeddb_origin: None,
    },
    Provider {
        key: "instagram",
        host_suffix: ".instagram.com",
        session_cookie_names: &["sessionid"],
        indexeddb_origin: None,
    },
    Provider {
        key: "messenger",
        host_suffix: ".messenger.com",
        session_cookie_names: &["xs", "c_user"],
        indexeddb_origin: None,
    },
];

/// Resolve the shared CEF cookies SQLite path from the env var.
///
/// Returns `None` if the env var is unset or empty. We do **not** try to
/// guess a platform-specific default here: the Tauri shell is the only
/// component that authoritatively knows the bundle identifier + cache
/// directory, and letting it configure us keeps dev/test/ci variants
/// (custom `OPENHUMAN_WORKSPACE`, renamed bundle) working without
/// special-casing.
fn cookies_db_path() -> Option<PathBuf> {
    let value = std::env::var(COOKIES_DB_ENV).ok()?;
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

/// Detect which supported webview providers have a live login in the
/// shared CEF cookie store.
///
/// Returns a JSON object keyed by provider slug, value `true` when at
/// least one known session cookie is present for that provider. Every
/// provider in [`PROVIDERS`] is present in the result, even when
/// `false` — the welcome agent uses `false` entries to decide what to
/// offer.
///
/// This never fails: missing env var, locked DB, schema drift — all
/// map to "everything false." The welcome snapshot is load-bearing on
/// first-run and must always build.
pub fn detect_webview_logins() -> Value {
    let mut out = serde_json::Map::with_capacity(PROVIDERS.len());
    for p in PROVIDERS {
        out.insert(p.key.to_string(), Value::Bool(false));
    }

    let Some(path) = cookies_db_path() else {
        tracing::debug!(
            env = COOKIES_DB_ENV,
            "[webview_accounts] cookies DB env var not set — reporting all providers as logged_out"
        );
        return Value::Object(out);
    };
    if !path.exists() {
        // Don't log the absolute path — it can include a username under
        // /Users/<name>/... or /home/<name>/... — log the env key only.
        tracing::debug!(
            env = COOKIES_DB_ENV,
            "[webview_accounts] cookies DB path does not exist — reporting all providers as logged_out"
        );
        return Value::Object(out);
    }

    // URI form with `mode=ro&immutable=1&nolock=1` is required because
    // CEF keeps an exclusive lock on the live cookies file; `immutable`
    // tells SQLite to skip the WAL and lock dance and read pages
    // directly. We don't care about concurrent writes from CEF — a
    // stale read is fine for a "has the user logged in" heuristic.
    //
    // The path component of a SQLite file: URI must be percent-encoded
    // per <https://sqlite.org/uri.html> — otherwise spaces (common in
    // macOS `/Users/John Doe/...`), `?`, `#`, `%`, and Windows `\`
    // separators would break parsing and the open silently fails.
    let uri = format!(
        "file:{}?mode=ro&immutable=1&nolock=1",
        sqlite_uri_path(&path)
    );
    let conn = match Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    ) {
        Ok(c) => c,
        Err(err) => {
            tracing::debug!(
                env = COOKIES_DB_ENV,
                error = %err,
                "[webview_accounts] failed to open cookies DB — reporting all providers as logged_out"
            );
            return Value::Object(out);
        }
    };

    // Resolve the IndexedDB root once. `path` points at the Cookies SQLite
    // file inside the Default profile (`{profile}/Default/Cookies`); the
    // IndexedDB store sits next to it at `{profile}/Default/IndexedDB/`.
    let indexeddb_root = path.parent().map(|p| p.join("IndexedDB"));

    for p in PROVIDERS {
        let cookie_match = provider_has_session_cookie(&conn, p);
        let indexeddb_match = match (p.indexeddb_origin, indexeddb_root.as_deref()) {
            (Some(origin), Some(root)) => provider_has_indexeddb(root, origin),
            _ => false,
        };
        let logged_in = cookie_match || indexeddb_match;
        tracing::debug!(
            provider = p.key,
            cookie_match,
            indexeddb_match,
            logged_in,
            "[webview_accounts] probed provider login state"
        );
        out.insert(p.key.to_string(), Value::Bool(logged_in));
    }

    Value::Object(out)
}

/// Return `true` when Chromium has a non-empty IndexedDB store for the
/// given origin (e.g. `https_web.telegram.org_0`). Providers like Telegram
/// Web store their auth blob (`dc1_auth_key`, etc.) here rather than in
/// cookies, so cookie presence is not a reliable login signal for them.
///
/// Chromium lays IndexedDB out as `<root>/<origin>.indexeddb.leveldb/`
/// containing LevelDB sst/log files. We treat "directory exists + has at
/// least one file" as logged-in. False positives are theoretically possible
/// (a previous session that was signed out but left files behind) — in
/// practice Telegram Web clears the directory on sign-out, so this is a
/// reasonable heuristic until a richer probe is wired.
fn provider_has_indexeddb(root: &std::path::Path, origin: &str) -> bool {
    let dir = root.join(format!("{origin}.indexeddb.leveldb"));
    let entries = match std::fs::read_dir(&dir) {
        Ok(it) => it,
        Err(_) => return false,
    };
    entries.flatten().next().is_some()
}

/// Return `true` when the cookie DB has at least one row whose host_key
/// ends with `host_suffix` and whose name is one of the provider's
/// session-cookie names. Any SQL failure maps to `false`.
fn provider_has_session_cookie(conn: &Connection, provider: &Provider) -> bool {
    if provider.session_cookie_names.is_empty() {
        return false;
    }
    let placeholders = provider
        .session_cookie_names
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT 1 FROM cookies \
         WHERE host_key LIKE ?1 ESCAPE '\\' \
         AND name IN ({placeholders}) \
         LIMIT 1"
    );

    // Escape SQL-LIKE metacharacters in the suffix so a provider entry
    // with `_` or `%` can't silently widen the match. All current
    // entries are plain hostnames but future additions might not be.
    let like_pattern = format!("%{}", escape_like(provider.host_suffix));

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(err) => {
            tracing::debug!(
                provider = provider.key,
                error = %err,
                "[webview_accounts] prepare cookies query failed"
            );
            return false;
        }
    };
    let mut params: Vec<&dyn rusqlite::ToSql> =
        Vec::with_capacity(1 + provider.session_cookie_names.len());
    params.push(&like_pattern);
    for name in provider.session_cookie_names {
        params.push(name);
    }
    match stmt.exists(params.as_slice()) {
        Ok(found) => found,
        Err(err) => {
            tracing::debug!(
                provider = provider.key,
                error = %err,
                "[webview_accounts] cookies query execution failed"
            );
            false
        }
    }
}

/// Encode a filesystem path for use as the path component of a SQLite
/// `file:` URI.
///
/// Per <https://sqlite.org/uri.html>: backslashes (Windows) become
/// forward slashes, then the path is percent-encoded so that spaces,
/// `?`, `#`, and literal `%` don't get reinterpreted as URI syntax.
/// We use `urlencoding::encode` and then put `/` separators back —
/// `urlencoding` is RFC-3986-strict and would otherwise escape every
/// `/` in the path, which SQLite doesn't want.
fn sqlite_uri_path(path: &std::path::Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    urlencoding::encode(&raw).replace("%2F", "/")
}

fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::sync::{Mutex, MutexGuard};
    use tempfile::TempDir;

    /// Serialise tests that mutate `COOKIES_DB_ENV`. Rust runs tests in
    /// parallel by default, and `std::env::set_var` is process-global —
    /// without this lock two tests can race and observe each other's
    /// env mutations. Using a plain `Mutex` rather than pulling in
    /// `serial_test` keeps the dev-deps surface flat.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire the env lock for the duration of a test. Recovers from a
    /// poisoned mutex (a previous test panicked) so a single failure
    /// doesn't cascade into "every other test panics on lock".
    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn make_cookies_db(path: &std::path::Path, rows: &[(&str, &str)]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE cookies (\
                 host_key TEXT NOT NULL,\
                 name     TEXT NOT NULL,\
                 value    TEXT NOT NULL\
             );",
        )
        .unwrap();
        for (host, name) in rows {
            conn.execute(
                "INSERT INTO cookies(host_key, name, value) VALUES (?1, ?2, '')",
                params![host, name],
            )
            .unwrap();
        }
    }

    /// Guard: results always cover every provider, even when the DB is
    /// missing. The welcome snapshot depends on this invariant.
    #[test]
    fn missing_env_returns_all_false() {
        let _lock = lock_env();
        std::env::remove_var(COOKIES_DB_ENV);
        let v = detect_webview_logins();
        let obj = v.as_object().expect("object");
        for p in PROVIDERS {
            assert_eq!(obj[p.key], Value::Bool(false), "provider {}", p.key);
        }
    }

    #[test]
    fn detects_whatsapp_via_session_cookie() {
        // After the Phase 0 roster curation, `gmail` is no longer a webview
        // provider (Google's anti-automation blocks CEF reliably). WhatsApp
        // is the canonical "high-value, no-API" provider, so this test
        // exercises the same code path the old `detects_gmail_via_sid_cookie`
        // covered — just keyed off `web.whatsapp.com` + `wa_ul` instead.
        let _lock = lock_env();
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("Cookies");
        make_cookies_db(&db, &[("web.whatsapp.com", "wa_ul")]);
        std::env::set_var(COOKIES_DB_ENV, &db);
        let v = detect_webview_logins();
        assert_eq!(v["whatsapp"], Value::Bool(true));
        assert_eq!(v["slack"], Value::Bool(false));
        std::env::remove_var(COOKIES_DB_ENV);
    }

    #[test]
    fn detects_slack_and_linkedin() {
        let _lock = lock_env();
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("Cookies");
        make_cookies_db(
            &db,
            &[("workspace.slack.com", "d"), (".linkedin.com", "li_at")],
        );
        std::env::set_var(COOKIES_DB_ENV, &db);
        let v = detect_webview_logins();
        assert_eq!(v["slack"], Value::Bool(true));
        assert_eq!(v["linkedin"], Value::Bool(true));
        // Whatsapp should NOT light up from slack/linkedin cookies.
        assert_eq!(v["whatsapp"], Value::Bool(false));
        std::env::remove_var(COOKIES_DB_ENV);
    }

    /// Analytics / non-session cookies on a supported provider host must
    /// not register as a login — only the curated session-cookie names
    /// count. Uses linkedin since the gmail equivalent (NID on google.com)
    /// no longer maps to a tracked provider after the roster curation.
    #[test]
    fn ignores_non_session_cookies() {
        let _lock = lock_env();
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("Cookies");
        // `bcookie` and `lang` are LinkedIn marketing/analytics cookies —
        // not in `PROVIDERS["linkedin"].session_cookie_names`.
        make_cookies_db(
            &db,
            &[(".linkedin.com", "bcookie"), (".linkedin.com", "lang")],
        );
        std::env::set_var(COOKIES_DB_ENV, &db);
        let v = detect_webview_logins();
        assert_eq!(v["linkedin"], Value::Bool(false));
        std::env::remove_var(COOKIES_DB_ENV);
    }

    #[test]
    fn detects_telegram_via_indexeddb_when_no_session_cookie() {
        // Telegram Web stores its session entirely in IndexedDB
        // (`dc1_auth_key`, etc.) — no cookies. The probe must still report
        // logged_in=true via the IndexedDB-folder existence fallback.
        let _lock = lock_env();
        let tmp = TempDir::new().unwrap();
        let cookies_db = tmp.path().join("Default").join("Cookies");
        std::fs::create_dir_all(cookies_db.parent().unwrap()).unwrap();
        // Empty cookies DB — no Telegram session cookie at all.
        make_cookies_db(&cookies_db, &[]);

        // Populate the IndexedDB directory Chromium would create for
        // `https://web.telegram.org`.
        let idb = tmp
            .path()
            .join("Default")
            .join("IndexedDB")
            .join("https_web.telegram.org_0.indexeddb.leveldb");
        std::fs::create_dir_all(&idb).unwrap();
        std::fs::write(idb.join("CURRENT"), b"MANIFEST-000001\n").unwrap();

        std::env::set_var(COOKIES_DB_ENV, &cookies_db);
        let v = detect_webview_logins();
        assert_eq!(v["telegram"], Value::Bool(true));
        // Sanity: providers without IndexedDB markers stay false.
        assert_eq!(v["whatsapp"], Value::Bool(false));
        std::env::remove_var(COOKIES_DB_ENV);
    }

    #[test]
    fn empty_telegram_indexeddb_dir_is_not_a_login() {
        // A leftover empty IndexedDB directory (e.g. created by Chromium
        // before navigation completed, or after a sign-out clearing pass)
        // must not be reported as a login.
        let _lock = lock_env();
        let tmp = TempDir::new().unwrap();
        let cookies_db = tmp.path().join("Default").join("Cookies");
        std::fs::create_dir_all(cookies_db.parent().unwrap()).unwrap();
        make_cookies_db(&cookies_db, &[]);
        let idb = tmp
            .path()
            .join("Default")
            .join("IndexedDB")
            .join("https_web.telegram.org_0.indexeddb.leveldb");
        std::fs::create_dir_all(&idb).unwrap();
        // Empty directory — no LevelDB files.

        std::env::set_var(COOKIES_DB_ENV, &cookies_db);
        let v = detect_webview_logins();
        assert_eq!(v["telegram"], Value::Bool(false));
        std::env::remove_var(COOKIES_DB_ENV);
    }

    #[test]
    fn empty_env_is_same_as_missing() {
        let _lock = lock_env();
        std::env::set_var(COOKIES_DB_ENV, "");
        let v = detect_webview_logins();
        // Every provider should be present + false when the env var is empty.
        for p in PROVIDERS {
            assert_eq!(v[p.key], Value::Bool(false), "provider {}", p.key);
        }
        std::env::remove_var(COOKIES_DB_ENV);
    }

    #[test]
    fn nonexistent_path_returns_all_false() {
        let _lock = lock_env();
        std::env::set_var(COOKIES_DB_ENV, "/tmp/does-not-exist/Cookies");
        let v = detect_webview_logins();
        for p in PROVIDERS {
            assert_eq!(v[p.key], Value::Bool(false), "provider {}", p.key);
        }
        std::env::remove_var(COOKIES_DB_ENV);
    }

    #[test]
    fn corrupt_db_returns_all_false() {
        let _lock = lock_env();
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("Cookies");
        std::fs::write(&db, b"not a sqlite file").unwrap();
        std::env::set_var(COOKIES_DB_ENV, &db);
        let v = detect_webview_logins();
        for p in PROVIDERS {
            assert_eq!(v[p.key], Value::Bool(false));
        }
        std::env::remove_var(COOKIES_DB_ENV);
    }

    /// macOS users often have a space in their username
    /// (`/Users/John Doe/...`); without percent-encoding, the SQLite
    /// `file:` URI fails to parse and we'd silently report all-false.
    #[test]
    fn detects_cookies_when_path_contains_spaces() {
        let _lock = lock_env();
        let tmp = TempDir::new().unwrap();
        let dir_with_space = tmp.path().join("dir with space");
        std::fs::create_dir_all(&dir_with_space).unwrap();
        let db = dir_with_space.join("Cookies");
        // Use a still-tracked provider's session cookie; gmail was removed
        // from PROVIDERS during the Phase 0 roster curation.
        make_cookies_db(&db, &[("web.whatsapp.com", "wa_ul")]);
        std::env::set_var(COOKIES_DB_ENV, &db);
        let v = detect_webview_logins();
        assert_eq!(v["whatsapp"], Value::Bool(true));
        std::env::remove_var(COOKIES_DB_ENV);
    }

    #[test]
    fn sqlite_uri_path_encodes_reserved_chars() {
        use std::path::Path;
        // Spaces and percents inside the path get encoded; slashes
        // remain literal so SQLite can parse the path component.
        assert_eq!(
            sqlite_uri_path(Path::new("/Users/John Doe/Cookies")),
            "/Users/John%20Doe/Cookies"
        );
        assert_eq!(
            sqlite_uri_path(Path::new("/tmp/100%off/Cookies")),
            "/tmp/100%25off/Cookies"
        );
    }

    #[test]
    fn escape_like_escapes_metachars() {
        assert_eq!(escape_like("ab_cd%ef\\gh"), "ab\\_cd\\%ef\\\\gh");
        assert_eq!(escape_like("plain.host.com"), "plain.host.com");
    }
}
