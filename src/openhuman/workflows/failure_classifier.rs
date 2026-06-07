//! T-4 (Phase 2.5 Trust UX) — classify a workflow run's terminal
//! failure into a structured [`FailureReason`].
//!
//! Called from the workflow executor at the moment a step flips to
//! `RunStatus::Failed`. Pattern-matches the `error` string against the
//! seven known failure modes (see [`FailureReason`]); falls through to
//! `Unknown { raw_detail }` + emits drift telemetry so unrecognised
//! signals get noticed before users do.
//!
//! Mirror of the design pattern in
//! [`crate::openhuman::composio::tool_errors::classify_composio_error`]
//! — string-pattern heuristics with a tracked Unknown fall-through.
//! The catalog is deliberately small + stable so the UI renderer can
//! match exhaustively.

use crate::openhuman::workflows::types::FailureReason;

/// The OpenHuman backend's canonical model tier list. Returned inside
/// `FailureReason::ModelUnavailable.valid_tiers` so the UI's fix-it
/// action can populate a model-picker. Kept in sync with
/// [`crate::openhuman::inference::provider::factory::is_known_openhuman_tier`].
const VALID_TIERS: &[&str] = &[
    "reasoning-v1",
    "chat-v1",
    "agentic-v1",
    "coding-v1",
    "reasoning-quick-v1",
    "summarization-v1",
];

/// Classify the executor's terminal error string into a structured
/// [`FailureReason`]. Conservative — patterns are matched explicitly,
/// and anything unrecognised returns `Unknown` (and emits drift
/// telemetry through the existing observability surface).
///
/// `narrative_chars` is the size of the agent's final text output —
/// only consulted for the `AgentNarratedWithoutActing` variant which
/// surfaces it in the renderer.
pub fn classify_failure(error: &str, narrative_chars: u32) -> FailureReason {
    let lower = error.to_lowercase();

    // F-21's exact signature — the executor's flip-to-Failed code
    // path emits "agent narrated next-action intent in N chars of
    // text without emitting any tool_use blocks" verbatim.
    if lower.contains("narrated next-action intent")
        || lower.contains("without emitting any tool_use blocks")
    {
        return FailureReason::AgentNarratedWithoutActing { narrative_chars };
    }

    // The OpenHuman backend's exact 400 string when the resolved model
    // isn't in the tier list:
    //   `Model 'claude-opus-4-7' is not available. Use GET /openai/v1/models...`
    if let Some(model_tried) = extract_model_unavailable(error) {
        return FailureReason::ModelUnavailable {
            model_tried,
            valid_tiers: VALID_TIERS.iter().map(|s| s.to_string()).collect(),
        };
    }

    // LLM provider 401s — surface the provider distinctly so the
    // fix-it action can deep-link to the right Settings panel.
    // Anthropic's exact error message:
    //   {"error":{"code":"authentication_error","message":"Invalid
    //   Anthropic API Key", ...}}
    if lower.contains("invalid anthropic api key") {
        return FailureReason::LlmAuthFailed {
            provider: "anthropic".to_string(),
        };
    }
    if lower.contains("invalid openai api key") || lower.contains("incorrect api key provided") {
        return FailureReason::LlmAuthFailed {
            provider: "openai".to_string(),
        };
    }

    // F-19 / F-20 structured composio block — when surfaced by F-16's
    // `composio_execute` honesty path, the error contains the
    // rendered block which includes `kind: auth_failed` /
    // `kind: invalid_slug_shape` / `kind: upstream_provider_error`.
    // Match those kind labels first since they're stable.
    if error.contains("kind: auth_failed") {
        // Provider extraction: the rendered detail typically names
        // the toolkit. Best-effort — fall back to "(unknown)" rather
        // than skipping the classification entirely.
        let provider =
            extract_provider_from_composio_error(error).unwrap_or_else(|| "(unknown)".to_string());
        return FailureReason::ConnectionExpired { provider };
    }
    if error.contains("kind: invalid_slug_shape") {
        let slug =
            extract_slug_from_composio_error(error).unwrap_or_else(|| "(unknown)".to_string());
        return FailureReason::ToolSlugInvalid { slug };
    }
    if error.contains("kind: upstream_provider_error") {
        let tool =
            extract_tool_from_composio_error(error).unwrap_or_else(|| "(unknown)".to_string());
        let detail = extract_detail_from_composio_error(error).unwrap_or_else(|| error.to_string());
        return FailureReason::ComposioUpstreamRejected { tool, detail };
    }

    // Anything else — record drift telemetry so a future failure mode
    // we haven't catalogued shows up in observability. The user still
    // gets the raw detail in the renderer.
    crate::core::observability::record_classifier_drift(
        crate::core::observability::ToolErrorClassifierSource::Composio,
        error,
    );
    FailureReason::Unknown {
        raw_detail: error.to_string(),
    }
}

// ── helpers ────────────────────────────────────────────────────────

/// Extract the model name from the backend's "Model '<X>' is not
/// available" error. Returns None when the string doesn't match the
/// expected shape.
fn extract_model_unavailable(error: &str) -> Option<String> {
    let lower = error.to_lowercase();
    let needle = "model '";
    let start = lower.find(needle)? + needle.len();
    let rest = &error[start..];
    let end = rest.find('\'')?;
    let model = &rest[..end];
    if !lower.contains("is not available") && !lower.contains("not available") {
        return None;
    }
    Some(model.to_string())
}

/// Pull `tool: <SLUG>` from the rendered F-19/F-20 composio error
/// block. The block format is:
///
/// ```text
/// ⚠ Composio tool error
/// tool: SLACK_SEND_MESSAGE
/// kind: upstream_provider_error
/// detail: ...
/// suggestion: ...
/// ```
fn extract_tool_from_composio_error(error: &str) -> Option<String> {
    extract_labeled_field(error, "tool:")
}

fn extract_detail_from_composio_error(error: &str) -> Option<String> {
    extract_labeled_field(error, "detail:")
}

/// Best-effort: the slug for `invalid_slug_shape` is in the `tool:`
/// line of the rendered block. Same extractor as `tool:`.
fn extract_slug_from_composio_error(error: &str) -> Option<String> {
    extract_labeled_field(error, "tool:")
}

/// Provider extraction for `kind: auth_failed`: the rendered block's
/// `detail:` line usually names the toolkit (e.g. "Composio reported
/// Gmail connection unauthorized"). Try the tool slug first (which
/// carries the toolkit as its prefix), then a substring search.
fn extract_provider_from_composio_error(error: &str) -> Option<String> {
    if let Some(tool) = extract_labeled_field(error, "tool:") {
        // Slug shape is TOOLKIT_VERB_OBJECT — first underscore-
        // separated token is the toolkit.
        if let Some(prefix) = tool.split('_').next() {
            if !prefix.is_empty() {
                return Some(prefix.to_lowercase());
            }
        }
    }
    None
}

/// Generic line-prefix extractor for the rendered composio block.
/// Reads until end-of-line and trims.
fn extract_labeled_field(error: &str, label: &str) -> Option<String> {
    for line in error.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(label) {
            let value = rest.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_narration_loop_signature() {
        let err = "agent narrated next-action intent in 340 chars of text without emitting any tool_use blocks. Workflow granted 2 action connection(s) but the agent made zero successful tool calls — the run accomplished nothing measurable.";
        let result = classify_failure(err, 340);
        assert_eq!(
            result,
            FailureReason::AgentNarratedWithoutActing {
                narrative_chars: 340
            }
        );
    }

    #[test]
    fn classify_model_unavailable_extracts_model_name() {
        let err = "node `n1` failed: agent_prompt step failed: OpenHuman API error (400 Bad Request): {\"success\":false,\"error\":\"Model 'claude-opus-4-7' is not available. Use GET /openai/v1/models to list available models.\"}";
        match classify_failure(err, 0) {
            FailureReason::ModelUnavailable {
                model_tried,
                valid_tiers,
            } => {
                assert_eq!(model_tried, "claude-opus-4-7");
                assert!(valid_tiers.iter().any(|t| t == "agentic-v1"));
                assert!(valid_tiers.iter().any(|t| t == "reasoning-v1"));
            }
            other => panic!("expected ModelUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn classify_anthropic_auth_failed() {
        let err = "anthropic API error (401 Unauthorized): {\"error\":{\"code\":\"authentication_error\",\"message\":\"Invalid Anthropic API Key\",\"type\":\"invalid_request_error\"}}";
        assert_eq!(
            classify_failure(err, 0),
            FailureReason::LlmAuthFailed {
                provider: "anthropic".to_string()
            }
        );
    }

    #[test]
    fn classify_openai_auth_failed() {
        let err = "openai API error 401: Incorrect API key provided: sk-...";
        assert_eq!(
            classify_failure(err, 0),
            FailureReason::LlmAuthFailed {
                provider: "openai".to_string()
            }
        );
    }

    #[test]
    fn classify_composio_upstream_rejected_extracts_tool_and_detail() {
        // F-16's rendered block format
        let err = "⚠ Composio tool error\ntool: SLACK_SEND_MESSAGE\nkind: upstream_provider_error\ndetail: Use markdown_text for normal content, or fallback_text with blocks. on parameter ``\nsuggestion: The connected provider rejected the request.";
        match classify_failure(err, 0) {
            FailureReason::ComposioUpstreamRejected { tool, detail } => {
                assert_eq!(tool, "SLACK_SEND_MESSAGE");
                assert!(detail.contains("markdown_text"));
            }
            other => panic!("expected ComposioUpstreamRejected, got {other:?}"),
        }
    }

    #[test]
    fn classify_composio_auth_failed_extracts_provider_from_slug() {
        let err = "⚠ Composio tool error\ntool: GMAIL_SEND_EMAIL\nkind: auth_failed\ndetail: token expired\nsuggestion: reconnect";
        assert_eq!(
            classify_failure(err, 0),
            FailureReason::ConnectionExpired {
                provider: "gmail".to_string()
            }
        );
    }

    #[test]
    fn classify_composio_invalid_slug_shape() {
        let err = "⚠ Composio tool error\ntool: linkedin\nkind: invalid_slug_shape\ndetail: received `linkedin`\nsuggestion: use uppercase slug";
        assert_eq!(
            classify_failure(err, 0),
            FailureReason::ToolSlugInvalid {
                slug: "linkedin".to_string()
            }
        );
    }

    #[test]
    fn classify_unknown_returns_raw_detail() {
        let err = "some completely unrecognised error message we've never seen";
        match classify_failure(err, 0) {
            FailureReason::Unknown { raw_detail } => {
                assert_eq!(raw_detail, err);
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn extract_model_unavailable_returns_none_on_unrelated_string() {
        assert!(extract_model_unavailable("Model 'foo' was fine").is_none());
        assert!(extract_model_unavailable("nothing here").is_none());
    }

    #[test]
    fn extract_labeled_field_finds_tool_line() {
        let err = "header\n  tool: GMAIL_SEND_EMAIL\nother: stuff";
        assert_eq!(
            extract_labeled_field(err, "tool:").as_deref(),
            Some("GMAIL_SEND_EMAIL")
        );
    }
}
