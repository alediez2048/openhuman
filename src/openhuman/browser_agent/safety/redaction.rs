//! F3-6 chunk 4a — redaction policy for audit log args + URLs.
//!
//! Applies a sweeping rewrite to two surfaces:
//!
//! 1. **URLs** — query parameters whose name matches the sensitive
//!    pattern (`token|key|secret|auth|password|session|sid`) are
//!    stripped from any URL the audit log persists. Repro: an OAuth
//!    callback URL `?code=ABC123&state=...` will leave its bearer
//!    code on disk for 30 days under the F3-6 chunk 2 retention
//!    window unless we strip it at write time.
//!
//! 2. **JSON args** — any string value whose adjacent key matches
//!    the sensitive-field pattern (`password|ssn|social.?security|
//!    tax.?id|account.?number|card.?number|cvv|secret|token|api.?key|
//!    bearer|cookie|set.cookie|authorization`) is rewritten to
//!    `[REDACTED:<key>]`. The counter returned alongside the
//!    rewritten string lands in the `redacted_fields_count` audit-log
//!    column so the run-detail UI can show that redaction ran.
//!
//! The screenshot-pixel side of F3-6 chunk 4 (black-bar overlay over
//! bounding boxes of input[type=password] elements) needs an image
//! processing dep and lands in chunk 4b.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

/// Case-insensitive match against the sensitive-key pattern. Anchored
/// on substring so `MyApiKey`, `apiKey`, `api-key`, `api_key` all
/// match.
static SENSITIVE_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(?i)",
        r"password|ssn|social.?security|tax.?id|account.?number|",
        r"card.?number|cvv|secret|token|api.?key|bearer|cookie|",
        r"authorization|x.?auth|session.?id|sid",
    ))
    .expect("sensitive key regex compiles")
});

/// Case-insensitive match for URL query parameter names that should
/// have their values stripped. Slightly tighter than `SENSITIVE_KEY_RE`
/// since URL params have very different lexicon — `code` is sensitive
/// in an OAuth callback but isn't in JSON keys.
static SENSITIVE_QUERY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)token|key|secret|auth|password|session|sid|code|access").expect(
        "sensitive query param regex compiles",
    )
});

/// Strip query parameter values for params whose name matches the
/// sensitive pattern. Replaces with the literal `REDACTED` token so
/// the URL stays parseable. Returns the rewritten URL + the count
/// of params whose value was redacted.
///
/// Non-URL inputs are returned unchanged with `redacted = 0`.
pub fn redact_url(url: &str) -> (String, u32) {
    let Some(query_start) = url.find('?') else {
        return (url.to_string(), 0);
    };
    let (base, query) = url.split_at(query_start + 1);
    let mut redacted = 0;
    let new_query: Vec<String> = query
        .split('&')
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            if SENSITIVE_QUERY_RE.is_match(k) && !v.is_empty() {
                redacted += 1;
                format!("{k}=REDACTED")
            } else {
                kv.to_string()
            }
        })
        .collect();
    (format!("{base}{}", new_query.join("&")), redacted)
}

/// Sweep a JSON value's string leaves and replace any whose adjacent
/// key matches the sensitive pattern. Returns `(rewritten_json,
/// redacted_count)`.
///
/// Walks objects recursively. Array elements are visited but they
/// have no key context — the sensitivity decision is made by the
/// outermost object key, not the array element's position.
pub fn redact_args(value: &Value) -> (Value, u32) {
    let mut count = 0;
    let new_value = walk(value, None, &mut count);
    (new_value, count)
}

/// Convenience: redact a JSON-serialised args string. Re-encodes the
/// rewritten value. Returns `(rewritten_json_string, redacted_count)`.
/// On parse failure (the args weren't JSON to begin with) returns the
/// input unchanged with `count = 0`.
pub fn redact_args_str(args_json: &str) -> (String, u32) {
    let Ok(parsed) = serde_json::from_str::<Value>(args_json) else {
        return (args_json.to_string(), 0);
    };
    let (rewritten, count) = redact_args(&parsed);
    let s = serde_json::to_string(&rewritten).unwrap_or_else(|_| args_json.to_string());
    (s, count)
}

fn walk(value: &Value, parent_key: Option<&str>, count: &mut u32) -> Value {
    match value {
        Value::Object(map) => {
            let mut new = serde_json::Map::with_capacity(map.len());
            for (k, v) in map.iter() {
                new.insert(k.clone(), walk(v, Some(k.as_str()), count));
            }
            Value::Object(new)
        }
        Value::Array(arr) => {
            // Array elements inherit the parent key context (e.g.
            // `passwords: ["a", "b"]` — both elements are sensitive).
            Value::Array(arr.iter().map(|v| walk(v, parent_key, count)).collect())
        }
        Value::String(_) | Value::Number(_) | Value::Bool(_) => {
            if let Some(key) = parent_key {
                if SENSITIVE_KEY_RE.is_match(key) {
                    *count += 1;
                    return Value::String(format!("[REDACTED:{key}]"));
                }
            }
            value.clone()
        }
        Value::Null => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redact_url_strips_sensitive_query_params() {
        let (u, n) = redact_url(
            "https://api.example.com/callback?code=abc123&state=xyz&token=secret",
        );
        assert!(u.contains("code=REDACTED"));
        assert!(u.contains("state=xyz")); // state not in sensitive pattern
        assert!(u.contains("token=REDACTED"));
        assert_eq!(n, 2);
    }

    #[test]
    fn redact_url_with_no_query_passes_through() {
        let (u, n) = redact_url("https://example.com/path/to/page");
        assert_eq!(u, "https://example.com/path/to/page");
        assert_eq!(n, 0);
    }

    #[test]
    fn redact_url_keeps_empty_value_params() {
        // `?foo=` with empty value — not redacted (nothing to hide).
        let (u, n) = redact_url("https://example.com/?token=");
        assert_eq!(u, "https://example.com/?token=");
        assert_eq!(n, 0);
    }

    #[test]
    fn redact_args_rewrites_password_field() {
        let input = json!({
            "username": "alice",
            "password": "hunter2",
            "remember_me": true
        });
        let (out, n) = redact_args(&input);
        assert_eq!(n, 1);
        assert_eq!(out["username"], "alice");
        assert_eq!(out["password"], "[REDACTED:password]");
        assert_eq!(out["remember_me"], true);
    }

    #[test]
    fn redact_args_walks_nested_objects() {
        let input = json!({
            "user": {
                "id": 42,
                "credentials": {
                    "api_key": "AKIA...",
                    "bearer_token": "eyJ..."
                }
            }
        });
        let (out, n) = redact_args(&input);
        assert_eq!(n, 2);
        assert_eq!(out["user"]["id"], 42);
        assert_eq!(out["user"]["credentials"]["api_key"], "[REDACTED:api_key]");
        assert_eq!(
            out["user"]["credentials"]["bearer_token"],
            "[REDACTED:bearer_token]"
        );
    }

    #[test]
    fn redact_args_handles_arrays_under_sensitive_key() {
        let input = json!({
            "passwords": ["one", "two", "three"]
        });
        let (out, n) = redact_args(&input);
        assert_eq!(n, 3);
        assert_eq!(out["passwords"][0], "[REDACTED:passwords]");
        assert_eq!(out["passwords"][1], "[REDACTED:passwords]");
        assert_eq!(out["passwords"][2], "[REDACTED:passwords]");
    }

    #[test]
    fn redact_args_matches_case_insensitively() {
        let input = json!({
            "MyAPIKey": "secret",
            "Authorization": "Bearer abc",
            "X-Auth-Token": "xyz"
        });
        let (_, n) = redact_args(&input);
        assert_eq!(n, 3);
    }

    #[test]
    fn redact_args_matches_underscored_and_hyphenated_variants() {
        // SSN with various punctuation; account_number; card-number.
        let input = json!({
            "ssn": "111-22-3333",
            "social_security": "111-22-3333",
            "account_number": "9876",
            "card-number": "4111111111111111",
            "cvv": "123"
        });
        let (_, n) = redact_args(&input);
        assert_eq!(n, 5);
    }

    #[test]
    fn redact_args_str_round_trips_json() {
        let input = r#"{"password":"hunter2","name":"alice"}"#;
        let (out, n) = redact_args_str(input);
        assert_eq!(n, 1);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["password"], "[REDACTED:password]");
        assert_eq!(parsed["name"], "alice");
    }

    #[test]
    fn redact_args_str_invalid_json_passes_through_unchanged() {
        let input = "not-json-{}}}";
        let (out, n) = redact_args_str(input);
        assert_eq!(out, input);
        assert_eq!(n, 0);
    }

    #[test]
    fn redact_args_leaves_innocuous_strings_alone() {
        let input = json!({
            "verb": "click",
            "element_id": 3,
            "user_id": "u-abc",
            "run_id": "r-xyz"
        });
        let (_, n) = redact_args(&input);
        assert_eq!(n, 0, "no sensitive keys in this payload");
    }
}
