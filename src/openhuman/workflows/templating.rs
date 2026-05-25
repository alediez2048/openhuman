//! F2-2: inter-node data passing via `{{...}}` templating (OQ-7).
//!
//! Single public entry point: [`substitute`]. Replaces two reference
//! shapes in arbitrary strings:
//!
//! - `{{trigger.<dotted.path>}}` — JSONPath against the run's trigger
//!   payload (webhook body, composio_event raw, channel_message raw).
//!   Bare `{{trigger}}` resolves to the whole payload as JSON.
//! - `{{node.<id>.output.<dotted.path>}}` — JSONPath against a prior
//!   node's output body. Bare `{{node.<id>.output}}` resolves to the
//!   whole body as JSON.
//!
//! Substitution semantics (intentionally narrow for F2-2):
//!
//! - Strings substitute as their inner value (no surrounding quotes).
//! - Numbers / booleans substitute as their JSON literal.
//! - Objects / arrays / null substitute as the compact JSON
//!   serialisation. This is the only shape that lets a Phase 2
//!   `http_request.body_template` interpolate a whole upstream-node
//!   output cleanly.
//! - Missing references (unknown node id, missing JSON path) leave
//!   the literal `{{...}}` token in place AND record a miss for the
//!   caller. The executor surfaces misses via the structured
//!   per-node error so the agent / user can fix the template.
//!
//! F2-2 does NOT implement filters (`{{x | upper}}`), conditionals,
//! or a full expression engine — OQ-7 locked those as deferred. F2-3
//! through F2-7 consume the surface as-is.

use serde_json::Value;
use std::collections::HashMap;

/// Inter-node + trigger data the templating substitution resolves
/// against. Populated by the executor between dispatch calls.
#[derive(Debug, Clone, Default)]
pub struct NodeContext {
    /// Whole trigger payload — webhook body for `Webhook`, raw event
    /// for `ComposioEvent`, raw message for `ChannelMessage`. JSON
    /// `Null` when the trigger doesn't carry a payload (Cron, Manual).
    pub trigger_payload: Value,
    /// Per-node output bodies, keyed by `Node.id`. The executor
    /// inserts after every successful `dispatch_node` call. Missing
    /// keys (failed nodes under `on_error = Continue` — F2-8) leave
    /// downstream `{{node.<failed>.output...}}` references unresolved.
    pub outputs: HashMap<String, Value>,
}

impl NodeContext {
    pub fn new(trigger_payload: Value) -> Self {
        Self {
            trigger_payload,
            outputs: HashMap::new(),
        }
    }

    pub fn record_output(&mut self, node_id: impl Into<String>, body: Value) {
        self.outputs.insert(node_id.into(), body);
    }
}

/// Outcome of a single [`substitute`] call. Carries the resolved
/// string AND the list of references that didn't resolve, so callers
/// can decide whether to fail loudly (the F-19/F-20-style structured
/// surface) or proceed with the raw token visible.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SubstituteOutcome {
    pub resolved: String,
    pub unresolved: Vec<UnresolvedRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedRef {
    /// The raw token as it appeared in the source string, including
    /// the surrounding `{{` `}}` braces.
    pub token: String,
    /// Human-readable reason — e.g. "no node with id `n2`",
    /// "trigger payload missing path `payload.user_id`". Empty when
    /// the reference shape itself was malformed.
    pub reason: String,
}

/// Substitute `{{trigger.*}}` and `{{node.<id>.output.*}}` references
/// in `raw`. Returns the rendered string plus any unresolved
/// references for callers to surface.
///
/// Implementation walks the string once, finds `{{...}}` spans, and
/// dispatches to the reference resolver. Non-templating curly braces
/// (e.g. `{ "x": 1 }`) are left intact because the matcher requires
/// exactly two opening + two closing braces with a `trigger.` or
/// `node.` prefix inside.
pub fn substitute(raw: &str, ctx: &NodeContext) -> SubstituteOutcome {
    let mut out = String::with_capacity(raw.len());
    let mut unresolved = Vec::new();
    let mut chars = raw.chars().peekable();
    let mut buffer = String::new();

    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'{') {
            chars.next();
            buffer.clear();
            // Capture up to the matching `}}`.
            let mut found_close = false;
            while let Some(inner) = chars.next() {
                if inner == '}' && chars.peek() == Some(&'}') {
                    chars.next();
                    found_close = true;
                    break;
                }
                buffer.push(inner);
            }
            if !found_close {
                // Unclosed `{{` — write the literal back and bail on
                // template parsing for this fragment.
                out.push_str("{{");
                out.push_str(&buffer);
                continue;
            }
            let token = format!("{{{{{}}}}}", buffer);
            let trimmed = buffer.trim();
            match resolve_ref(trimmed, ctx) {
                Ok(value) => out.push_str(&render_value(&value)),
                Err(reason) => {
                    unresolved.push(UnresolvedRef {
                        token: token.clone(),
                        reason,
                    });
                    out.push_str(&token);
                }
            }
        } else {
            out.push(c);
        }
    }

    SubstituteOutcome {
        resolved: out,
        unresolved,
    }
}

/// F2-3 extension: substitute templating references inside a JSON
/// [`Value`] — walks the structure and applies [`substitute`] to every
/// string leaf. Object keys are NOT templated (only values); this
/// matches the OQ-7 lock that templating is for *values*, not schema
/// key names. Numbers / booleans / nulls pass through unchanged.
///
/// Single-string-leaf semantics: when an entire string value is exactly
/// one `{{...}}` reference whose resolution is a structured value
/// (object / array / number / boolean / null), the leaf is REPLACED
/// with the structured value — not the string serialisation. This
/// lets `tool_call.args = { "user": "{{trigger.payload.user}}" }`
/// pass a structured user object through to the tool when the trigger
/// payload's `user` field is itself an object.
///
/// Strings containing other text in addition to the reference
/// (`"hi {{name}}"`) always substitute as a string (no type
/// promotion), matching `substitute`'s behaviour.
///
/// Returns the rewritten value AND any unresolved refs collected
/// across every leaf the walker visited.
pub fn substitute_json(value: &Value, ctx: &NodeContext) -> (Value, Vec<UnresolvedRef>) {
    let mut unresolved = Vec::new();
    let resolved = substitute_json_inner(value, ctx, &mut unresolved);
    (resolved, unresolved)
}

fn substitute_json_inner(
    value: &Value,
    ctx: &NodeContext,
    unresolved: &mut Vec<UnresolvedRef>,
) -> Value {
    match value {
        Value::String(s) => {
            // Detect the "single-reference-only" form so a JSON object
            // can flow through intact when the resolved value is also
            // a JSON object — without this, every templated arg
            // collapses to a string-serialisation of its value.
            if let Some(only_ref) = parse_single_ref(s) {
                match resolve_ref(only_ref, ctx) {
                    Ok(v) => return v,
                    Err(reason) => {
                        unresolved.push(UnresolvedRef {
                            token: s.clone(),
                            reason,
                        });
                        return Value::String(s.clone());
                    }
                }
            }
            // Mixed-content strings — fall back to `substitute`, which
            // returns a String regardless of the resolved value's type.
            let outcome = substitute(s, ctx);
            unresolved.extend(outcome.unresolved);
            Value::String(outcome.resolved)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| substitute_json_inner(v, ctx, unresolved))
                .collect(),
        ),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), substitute_json_inner(v, ctx, unresolved));
            }
            Value::Object(out)
        }
        // Numbers, bools, nulls — pass through unchanged.
        other => other.clone(),
    }
}

/// Return the inner ref body when `s` is exactly `{{...}}` with no
/// surrounding text — used by `substitute_json` to decide between
/// type-preserving substitution (single ref) and string substitution
/// (mixed content).
fn parse_single_ref(s: &str) -> Option<&str> {
    let trimmed = s.trim();
    let inner = trimmed.strip_prefix("{{")?.strip_suffix("}}")?;
    // Reject inner content that itself contains `}}` followed by more
    // text — that means the user wrote `{{a}}{{b}}` (two refs);
    // substitute_json's single-ref shortcut only fires for ONE ref.
    if inner.contains("}}") {
        return None;
    }
    Some(inner.trim())
}

/// Resolve one `{{...}}` body (without braces) against the context.
///
/// Supported shapes:
///   - `trigger`                         → whole payload
///   - `trigger.<path>`                  → JSONPath against payload
///   - `node.<id>.output`                → whole body of node `<id>`
///   - `node.<id>.output.<path>`         → JSONPath against body
fn resolve_ref(body: &str, ctx: &NodeContext) -> Result<Value, String> {
    if body.is_empty() {
        return Err("empty template reference `{{}}`".to_string());
    }
    let mut parts = body.split('.');
    let head = parts.next().unwrap();
    match head {
        "trigger" => {
            let rest: Vec<&str> = parts.collect();
            if rest.is_empty() {
                return Ok(ctx.trigger_payload.clone());
            }
            walk_path(&ctx.trigger_payload, &rest)
                .cloned()
                .ok_or_else(|| format!("trigger payload missing path `{}`", rest.join(".")))
        }
        "node" => {
            let node_id = parts
                .next()
                .ok_or_else(|| "`node` reference missing `<id>`".to_string())?;
            let output_keyword = parts.next();
            if output_keyword != Some("output") {
                return Err(format!(
                    "`node.{node_id}` reference must continue with `.output[.<path>...]`"
                ));
            }
            let body_ref = ctx
                .outputs
                .get(node_id)
                .ok_or_else(|| format!("no output recorded for node `{node_id}`"))?;
            let rest: Vec<&str> = parts.collect();
            if rest.is_empty() {
                return Ok(body_ref.clone());
            }
            walk_path(body_ref, &rest)
                .cloned()
                .ok_or_else(|| format!("node `{node_id}` output missing path `{}`", rest.join(".")))
        }
        other => Err(format!(
            "unknown template reference root `{other}` (expected `trigger` or `node`)"
        )),
    }
}

/// Walk `value` through a dotted path. Supports object key access
/// only — array indexing via `[N]` is deferred until a concrete use
/// case appears (OQ-7's lean kept the scope to JSON-pointer-style
/// dotted paths).
fn walk_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cursor = value;
    for segment in path {
        if segment.is_empty() {
            return None;
        }
        cursor = cursor.get(*segment)?;
    }
    Some(cursor)
}

/// Render a resolved `Value` as the string the template substitutes
/// in. Strings substitute as their inner content (no quotes);
/// everything else uses compact JSON serialisation so a templated
/// `http_request.body_template` can carry a whole upstream object.
fn render_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx_with_outputs(outputs: &[(&str, Value)]) -> NodeContext {
        let mut ctx = NodeContext::new(Value::Null);
        for (id, body) in outputs {
            ctx.record_output(*id, body.clone());
        }
        ctx
    }

    #[test]
    fn substitute_passes_through_strings_with_no_references() {
        let outcome = substitute(
            "hello world — plain text with { single braces } intact",
            &NodeContext::default(),
        );
        assert_eq!(
            outcome.resolved,
            "hello world — plain text with { single braces } intact"
        );
        assert!(outcome.unresolved.is_empty());
    }

    #[test]
    fn substitute_inlines_trigger_payload_string_field() {
        let ctx = NodeContext::new(json!({ "user": "jad", "score": 9 }));
        let outcome = substitute("hi {{trigger.user}}!", &ctx);
        assert_eq!(outcome.resolved, "hi jad!");
        assert!(outcome.unresolved.is_empty());
    }

    #[test]
    fn substitute_inlines_trigger_payload_number_field() {
        let ctx = NodeContext::new(json!({ "score": 9 }));
        let outcome = substitute("score={{trigger.score}}", &ctx);
        assert_eq!(outcome.resolved, "score=9");
    }

    #[test]
    fn substitute_inlines_whole_trigger_payload_as_json() {
        let ctx = NodeContext::new(json!({ "user": "jad" }));
        let outcome = substitute("body={{trigger}}", &ctx);
        assert_eq!(outcome.resolved, r#"body={"user":"jad"}"#);
    }

    #[test]
    fn substitute_inlines_node_output_deep_path() {
        let ctx = ctx_with_outputs(&[(
            "summarize",
            json!({ "result": { "headline": "all good", "items": 3 } }),
        )]);
        let outcome = substitute("Headline: {{node.summarize.output.result.headline}}", &ctx);
        assert_eq!(outcome.resolved, "Headline: all good");
    }

    #[test]
    fn substitute_inlines_whole_node_output_as_json() {
        let ctx = ctx_with_outputs(&[("classify", json!({ "score": 0.7 }))]);
        let outcome = substitute("body={{node.classify.output}}", &ctx);
        assert_eq!(outcome.resolved, r#"body={"score":0.7}"#);
    }

    #[test]
    fn substitute_records_unresolved_when_node_missing() {
        let ctx = NodeContext::default();
        let outcome = substitute("v={{node.missing.output.x}}", &ctx);
        assert_eq!(outcome.resolved, "v={{node.missing.output.x}}");
        assert_eq!(outcome.unresolved.len(), 1);
        assert_eq!(outcome.unresolved[0].token, "{{node.missing.output.x}}");
        assert!(outcome.unresolved[0].reason.contains("no output recorded"));
    }

    #[test]
    fn substitute_records_unresolved_when_trigger_path_missing() {
        let ctx = NodeContext::new(json!({ "user": "jad" }));
        let outcome = substitute("v={{trigger.missing.path}}", &ctx);
        assert_eq!(outcome.resolved, "v={{trigger.missing.path}}");
        assert_eq!(outcome.unresolved.len(), 1);
        assert!(outcome.unresolved[0].reason.contains("missing path"));
    }

    #[test]
    fn substitute_rejects_unknown_root() {
        let ctx = NodeContext::default();
        let outcome = substitute("v={{env.PATH}}", &ctx);
        assert_eq!(outcome.resolved, "v={{env.PATH}}");
        assert_eq!(outcome.unresolved.len(), 1);
        assert!(outcome.unresolved[0]
            .reason
            .contains("unknown template reference root"));
    }

    #[test]
    fn substitute_handles_multiple_refs_in_one_string() {
        let mut ctx = NodeContext::new(json!({ "id": "u-123" }));
        ctx.record_output("classify", json!({ "score": 0.91 }));
        let outcome = substitute(
            "user={{trigger.id}} score={{node.classify.output.score}}",
            &ctx,
        );
        assert_eq!(outcome.resolved, "user=u-123 score=0.91");
        assert!(outcome.unresolved.is_empty());
    }

    #[test]
    fn substitute_leaves_unclosed_template_token_alone() {
        // `{{trigger.user` with no closing braces — the parser should
        // fall through and emit the literal text without panicking.
        let outcome = substitute("hi {{trigger.user", &NodeContext::default());
        // We don't promise the exact passthrough shape — only that
        // the result contains the original text and we don't crash.
        assert!(outcome.resolved.contains("trigger.user"));
    }

    #[test]
    fn substitute_does_not_match_single_brace_pairs() {
        // The JSON-body templating use case needs literal `{ "x": 1 }`
        // to flow through untouched. Only `{{...}}` matches.
        let outcome = substitute(r#"body={ "x": 1 }"#, &NodeContext::default());
        assert_eq!(outcome.resolved, r#"body={ "x": 1 }"#);
        assert!(outcome.unresolved.is_empty());
    }

    #[test]
    fn substitute_rejects_empty_template_braces() {
        let outcome = substitute("v={{}}", &NodeContext::default());
        assert_eq!(outcome.resolved, "v={{}}");
        assert_eq!(outcome.unresolved.len(), 1);
        assert!(outcome.unresolved[0].reason.contains("empty template"));
    }

    #[test]
    fn substitute_rejects_node_ref_without_output_keyword() {
        let mut ctx = NodeContext::default();
        ctx.record_output("n1", json!({"x": 1}));
        let outcome = substitute("v={{node.n1.body}}", &ctx);
        assert_eq!(outcome.unresolved.len(), 1);
        assert!(outcome.unresolved[0]
            .reason
            .contains("must continue with `.output"));
    }

    #[test]
    fn render_value_strings_have_no_quotes() {
        assert_eq!(render_value(&json!("hello")), "hello");
    }

    #[test]
    fn render_value_objects_use_compact_json() {
        assert_eq!(render_value(&json!({"a": 1})), r#"{"a":1}"#);
    }

    // ── F2-3: substitute_json ──────────────────────────────────────────

    #[test]
    fn substitute_json_passes_through_non_string_leaves() {
        let value = json!({ "n": 42, "b": true, "x": null });
        let (out, unresolved) = substitute_json(&value, &NodeContext::default());
        assert_eq!(out, value);
        assert!(unresolved.is_empty());
    }

    #[test]
    fn substitute_json_substitutes_string_leaves() {
        let mut ctx = NodeContext::new(json!({ "user": "jad" }));
        ctx.record_output("n1", json!({ "score": 9 }));
        let value = json!({
            "greeting": "hi {{trigger.user}}",
            "score_text": "score={{node.n1.output.score}}"
        });
        let (out, unresolved) = substitute_json(&value, &ctx);
        assert!(unresolved.is_empty());
        assert_eq!(out["greeting"], json!("hi jad"));
        assert_eq!(out["score_text"], json!("score=9"));
    }

    #[test]
    fn substitute_json_single_ref_preserves_value_type() {
        // When the WHOLE string is exactly `{{...}}` and the resolved
        // value is a non-string JSON value, the leaf MUST become that
        // structured value — not its string serialisation. This lets
        // `tool_call.args = { count: "{{trigger.n}}" }` send a real
        // number, not `"42"`.
        let ctx = NodeContext::new(json!({ "n": 42 }));
        let value = json!({ "count": "{{trigger.n}}" });
        let (out, _) = substitute_json(&value, &ctx);
        assert_eq!(out, json!({ "count": 42 }));
    }

    #[test]
    fn substitute_json_single_ref_preserves_object_value() {
        let mut ctx = NodeContext::new(json!({}));
        ctx.record_output("classify", json!({ "score": 0.7, "label": "ok" }));
        let value = json!({ "classification": "{{node.classify.output}}" });
        let (out, _) = substitute_json(&value, &ctx);
        assert_eq!(
            out,
            json!({ "classification": { "score": 0.7, "label": "ok" } })
        );
    }

    #[test]
    fn substitute_json_walks_arrays() {
        let ctx = NodeContext::new(json!({ "u": "jad" }));
        let value = json!(["plain", "ref={{trigger.u}}"]);
        let (out, _) = substitute_json(&value, &ctx);
        assert_eq!(out, json!(["plain", "ref=jad"]));
    }

    #[test]
    fn substitute_json_walks_nested_objects() {
        let ctx = NodeContext::new(json!({ "uid": 7 }));
        let value = json!({
            "outer": { "inner": { "id": "{{trigger.uid}}" } }
        });
        let (out, _) = substitute_json(&value, &ctx);
        assert_eq!(out, json!({ "outer": { "inner": { "id": 7 } } }));
    }

    #[test]
    fn substitute_json_collects_unresolved_across_leaves() {
        let ctx = NodeContext::default();
        let value = json!({
            "a": "{{trigger.missing}}",
            "b": "{{node.absent.output.x}}"
        });
        let (_, unresolved) = substitute_json(&value, &ctx);
        assert_eq!(unresolved.len(), 2);
    }

    #[test]
    fn substitute_json_does_not_template_object_keys() {
        // OQ-7 lock: keys are schema; only values template.
        let ctx = NodeContext::new(json!({ "k": "renamed" }));
        let value = json!({ "{{trigger.k}}": "value" });
        let (out, _) = substitute_json(&value, &ctx);
        // Key must NOT have been substituted.
        assert!(out.as_object().unwrap().contains_key("{{trigger.k}}"));
    }
}
