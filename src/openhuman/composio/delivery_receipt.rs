//! T-1 (Phase 2.5 Trust UX) — classify a successful `composio_execute`
//! call into a structured [`DeliveryReceipt`] so the workflow run
//! captures user-renderable evidence of what actually happened.
//!
//! Called from
//! [`crate::openhuman::composio::tools::ComposioExecuteTool::execute`]
//! in the `resp.successful == true` branch. The result is published as
//! a [`crate::core::event_bus::DomainEvent::DeliveryReceiptObserved`]
//! event scoped to the active `session_id` (same scope key the F-16
//! `ToolExecutionCompleted` subscriber uses). The workflows executor's
//! recorder picks it up and persists it on the run step.
//!
//! ## Classification contract
//!
//! - Read-class tools (`*_LIST_*`, `*_FETCH_*`, `*_GET_*`, `*_SEARCH_*`)
//!   return `None` — they don't produce a side effect, no receipt.
//! - Write-class tools (`*_SEND_*`, `*_CREATE_*`, `*_UPDATE_*`,
//!   `*_POST_*`, `*_DELETE_*`) return `Some(receipt)`. Curated rules
//!   exist for the high-volume slugs (Gmail / Slack / GCal / Notion /
//!   Attio); everything else falls through to `Other { verb: <Verb> }`
//!   with the imperative form of the action verb so the UI stays
//!   honest about what kind of side effect it's looking at.
//! - Per OQ-T1-A: when a curated rule can't extract `recipient` from
//!   the dispatch arguments (schema drift, malformed call), emit the
//!   receipt with `recipient: None` rather than skipping it or
//!   inventing a placeholder.

use chrono::Utc;
use serde_json::Value;

use crate::openhuman::workflows::types::{DeliveryReceipt, SideEffectKind};

use super::types::ComposioExecuteResponse;

/// Classify a `composio_execute` call into a [`DeliveryReceipt`].
///
/// Returns `None` for read-class tools (no side effect to record) and
/// for tools that match no write-class pattern. Returns `Some(receipt)`
/// when the call produced a side effect the user might want to verify.
///
/// `arguments` is the raw object passed to `composio_execute` — i.e.
/// the value of the `arguments` JSON field. `response` is the full
/// Composio response envelope (we read `data` + optional fields off it
/// to build `message_id` + `link`).
pub fn classify(
    tool: &str,
    arguments: Option<&Value>,
    response: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    // Curated rules — order doesn't matter, slugs are disjoint.
    if let Some(receipt) = classify_gmail_send_email(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_slack_send_message(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_googlecalendar_create_event(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_notion_create_page(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_attio_create_record(tool, arguments, response) {
        return Some(receipt);
    }

    // Fall-through: catch-all for any other write-class slug.
    classify_generic_write(tool, arguments, response)
}

// ── Curated classifiers ────────────────────────────────────────────

fn classify_gmail_send_email(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "GMAIL_SEND_EMAIL" {
        return None;
    }
    let recipient = string_from_args(args, "recipient_email");
    let message_id = string_from_data(&resp.data, "id").or_else(|| message_id_from_response_id(resp));
    let link = message_id
        .as_ref()
        .map(|id| format!("https://mail.google.com/mail/u/0/#sent/{id}"));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::EmailSent,
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_slack_send_message(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    // Covers both the v2-style `SLACK_SEND_MESSAGE` slug (curated set)
    // and the legacy `SLACK_CHAT_POSTMESSAGE` slug the agent
    // occasionally picks from the v1 catalog.
    if tool != "SLACK_SEND_MESSAGE" && tool != "SLACK_CHAT_POSTMESSAGE" {
        return None;
    }
    let recipient = string_from_args(args, "channel");
    // Slack returns `ts` as the canonical message identifier on
    // chat.postMessage. We don't generate a deep link yet because that
    // requires the team id which isn't in the response envelope —
    // leave `link: None` and let a future ticket add it once we
    // surface team id alongside the connection.
    let message_id = string_from_data(&resp.data, "ts");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::MessagePosted {
            provider: "slack".to_string(),
        },
        recipient,
        message_id,
        link: None,
        at: Utc::now(),
    })
}

fn classify_googlecalendar_create_event(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "GOOGLECALENDAR_CREATE_EVENT" {
        return None;
    }
    let recipient = string_from_args(args, "summary");
    let message_id = string_from_data(&resp.data, "id");
    let link = string_from_data(&resp.data, "htmlLink");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::CalendarEventCreated,
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_notion_create_page(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "NOTION_CREATE_PAGE" {
        return None;
    }
    let recipient = string_from_args(args, "title").or_else(|| string_from_args(args, "parent_id"));
    let message_id = string_from_data(&resp.data, "id");
    let link = string_from_data(&resp.data, "url");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::FileCreated {
            provider: "notion".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_attio_create_record(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "ATTIO_CREATE_RECORD" {
        return None;
    }
    // Attio nests the human-friendly name under `values.name.0.value`
    // in many object types. Best-effort lookup; falls back to `None`
    // when the schema doesn't match (OQ-T1-A: emit the receipt anyway).
    let recipient = args.and_then(|a| {
        a.get("values")
            .and_then(|v| v.get("name"))
            .and_then(|n| n.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("value"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    });
    let message_id = string_from_data(&resp.data, "id");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::RecordCreated {
            provider: "attio".to_string(),
        },
        recipient,
        message_id,
        link: None,
        at: Utc::now(),
    })
}

// ── Generic write fall-through ─────────────────────────────────────

/// Classify a write-class slug that doesn't match a curated rule.
/// Returns `Some(Other { verb })` when the slug matches a write
/// pattern, `None` for reads.
fn classify_generic_write(
    tool: &str,
    _args: Option<&Value>,
    _resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    let verb = write_verb_from_slug(tool)?;
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::Other { verb },
        recipient: None,
        message_id: None,
        link: None,
        at: Utc::now(),
    })
}

/// Extract a verb from a write-class slug. Returns `None` for reads.
///
/// Heuristic on the tokens of a `TOOLKIT_VERB_OBJECT` slug:
///   - `*_SEND_*` → "Sent"
///   - `*_CREATE_*` → "Created"
///   - `*_UPDATE_*` → "Updated"
///   - `*_POST_*` → "Posted"
///   - `*_DELETE_*` / `*_REMOVE_*` → "Deleted"
///   - else → None (no side effect)
fn write_verb_from_slug(tool: &str) -> Option<String> {
    // Tokenise on '_' and check each token. We do NOT match on the
    // first token (the toolkit prefix) so `SLACK_REMOVE_USER` returns
    // "Deleted" not based on `SLACK` but on the explicit `REMOVE`.
    let mut tokens = tool.split('_');
    // skip the toolkit prefix
    let _ = tokens.next();
    for token in tokens {
        match token {
            "SEND" => return Some("Sent".to_string()),
            "CREATE" => return Some("Created".to_string()),
            "UPDATE" | "EDIT" | "MODIFY" => return Some("Updated".to_string()),
            "POST" => return Some("Posted".to_string()),
            "DELETE" | "REMOVE" => return Some("Deleted".to_string()),
            "ARCHIVE" => return Some("Archived".to_string()),
            "MOVE" => return Some("Moved".to_string()),
            _ => continue,
        }
    }
    None
}

// ── helpers ────────────────────────────────────────────────────────

fn string_from_args(args: Option<&Value>, key: &str) -> Option<String> {
    args.and_then(|a| a.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn string_from_data(data: &Value, key: &str) -> Option<String> {
    data.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

/// Some Composio responses surface a top-level `id` at the response
/// envelope rather than under `data`. Best-effort fallback so we
/// don't miss obvious message ids.
fn message_id_from_response_id(resp: &ComposioExecuteResponse) -> Option<String> {
    serde_json::to_value(resp)
        .ok()?
        .get("id")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn resp_with_data(data: Value) -> ComposioExecuteResponse {
        ComposioExecuteResponse {
            data,
            successful: true,
            error: None,
            cost_usd: 0.0,
            markdown_formatted: None,
        }
    }

    // ── curated: GMAIL_SEND_EMAIL ──

    #[test]
    fn classify_gmail_send_email_extracts_recipient_message_id_and_link() {
        let args = json!({
            "recipient_email": "alediez2408@gmail.com",
            "subject": "Morning brief 6/7",
            "body": "..."
        });
        let resp = resp_with_data(json!({ "id": "18f0c1d2a3b4c5d6", "threadId": "..." }));
        let receipt =
            classify("GMAIL_SEND_EMAIL", Some(&args), &resp).expect("should classify as receipt");
        assert_eq!(receipt.tool, "GMAIL_SEND_EMAIL");
        assert_eq!(receipt.side_effect_kind, SideEffectKind::EmailSent);
        assert_eq!(receipt.recipient.as_deref(), Some("alediez2408@gmail.com"));
        assert_eq!(receipt.message_id.as_deref(), Some("18f0c1d2a3b4c5d6"));
        assert_eq!(
            receipt.link.as_deref(),
            Some("https://mail.google.com/mail/u/0/#sent/18f0c1d2a3b4c5d6")
        );
    }

    #[test]
    fn classify_gmail_send_email_missing_recipient_emits_receipt_with_none() {
        // OQ-T1-A: extraction failure → recipient: None, not skip + not invent
        let resp = resp_with_data(json!({ "id": "abc" }));
        let receipt = classify("GMAIL_SEND_EMAIL", None, &resp)
            .expect("receipt must still be emitted even without args");
        assert_eq!(receipt.side_effect_kind, SideEffectKind::EmailSent);
        assert!(
            receipt.recipient.is_none(),
            "must not invent a placeholder recipient"
        );
        assert_eq!(receipt.message_id.as_deref(), Some("abc"));
    }

    // ── curated: SLACK_SEND_MESSAGE ──

    #[test]
    fn classify_slack_send_message_extracts_channel_and_ts() {
        let args = json!({ "channel": "C012345", "text": "hi", "markdown_text": "hi" });
        let resp = resp_with_data(json!({ "ts": "1717800000.000100", "channel": "C012345" }));
        let receipt = classify("SLACK_SEND_MESSAGE", Some(&args), &resp).unwrap();
        assert_eq!(
            receipt.side_effect_kind,
            SideEffectKind::MessagePosted {
                provider: "slack".to_string()
            }
        );
        assert_eq!(receipt.recipient.as_deref(), Some("C012345"));
        assert_eq!(receipt.message_id.as_deref(), Some("1717800000.000100"));
        // Slack link template needs team id — not yet wired; receipt
        // emits with link: None and a future ticket adds it.
        assert!(receipt.link.is_none());
    }

    #[test]
    fn classify_slack_chat_postmessage_alias_also_classifies() {
        let args = json!({ "channel": "U0AEVBF9CQH" });
        let resp = resp_with_data(json!({ "ts": "1.0" }));
        let receipt = classify("SLACK_CHAT_POSTMESSAGE", Some(&args), &resp).unwrap();
        assert_eq!(
            receipt.side_effect_kind,
            SideEffectKind::MessagePosted {
                provider: "slack".to_string()
            }
        );
    }

    // ── curated: GOOGLECALENDAR_CREATE_EVENT ──

    #[test]
    fn classify_googlecalendar_create_event_uses_summary_and_htmllink() {
        let args = json!({ "summary": "Standup", "start": {}, "end": {} });
        let resp = resp_with_data(json!({
            "id": "evt_123",
            "htmlLink": "https://calendar.google.com/event?eid=evt_123"
        }));
        let receipt = classify("GOOGLECALENDAR_CREATE_EVENT", Some(&args), &resp).unwrap();
        assert_eq!(
            receipt.side_effect_kind,
            SideEffectKind::CalendarEventCreated
        );
        assert_eq!(receipt.recipient.as_deref(), Some("Standup"));
        assert_eq!(receipt.message_id.as_deref(), Some("evt_123"));
        assert_eq!(
            receipt.link.as_deref(),
            Some("https://calendar.google.com/event?eid=evt_123")
        );
    }

    // ── curated: NOTION_CREATE_PAGE ──

    #[test]
    fn classify_notion_create_page_uses_title_and_url() {
        let args = json!({ "title": "Meeting notes", "parent_id": "page_abc" });
        let resp = resp_with_data(json!({
            "id": "pg_xyz",
            "url": "https://notion.so/pg_xyz"
        }));
        let receipt = classify("NOTION_CREATE_PAGE", Some(&args), &resp).unwrap();
        assert_eq!(
            receipt.side_effect_kind,
            SideEffectKind::FileCreated {
                provider: "notion".to_string()
            }
        );
        assert_eq!(receipt.recipient.as_deref(), Some("Meeting notes"));
        assert_eq!(receipt.link.as_deref(), Some("https://notion.so/pg_xyz"));
    }

    // ── curated: ATTIO_CREATE_RECORD ──

    #[test]
    fn classify_attio_create_record_extracts_nested_name() {
        let args = json!({
            "values": {
                "name": [{ "value": "Acme Corp", "active_from": "2026-06-07" }]
            }
        });
        let resp = resp_with_data(json!({ "id": "rec_acme" }));
        let receipt = classify("ATTIO_CREATE_RECORD", Some(&args), &resp).unwrap();
        assert_eq!(
            receipt.side_effect_kind,
            SideEffectKind::RecordCreated {
                provider: "attio".to_string()
            }
        );
        assert_eq!(receipt.recipient.as_deref(), Some("Acme Corp"));
        assert_eq!(receipt.message_id.as_deref(), Some("rec_acme"));
    }

    // ── generic write fall-through ──

    #[test]
    fn classify_uncurated_write_tool_falls_back_to_other_sent() {
        // OQ-T1-C: any write tool, not just composio, gets a receipt.
        let receipt = classify(
            "WIDGETS_SEND_FOO",
            Some(&json!({ "x": 1 })),
            &resp_with_data(json!({})),
        )
        .expect("write tool must produce a receipt");
        match receipt.side_effect_kind {
            SideEffectKind::Other { verb } => assert_eq!(verb, "Sent"),
            other => panic!("expected Other(Sent), got {other:?}"),
        }
        assert!(receipt.recipient.is_none());
    }

    #[test]
    fn classify_delete_action_uses_deleted_verb() {
        // OQ-T1-B: deletes ARE side effects, classified as Other(Deleted)
        let receipt = classify(
            "GMAIL_DELETE_MESSAGE",
            Some(&json!({ "id": "x" })),
            &resp_with_data(json!({})),
        )
        .expect("delete must produce a receipt");
        match receipt.side_effect_kind {
            SideEffectKind::Other { verb } => assert_eq!(verb, "Deleted"),
            other => panic!("expected Other(Deleted), got {other:?}"),
        }
    }

    #[test]
    fn classify_update_aliases_edit_and_modify() {
        for slug in &["FOO_UPDATE_BAR", "FOO_EDIT_BAR", "FOO_MODIFY_BAR"] {
            let receipt = classify(slug, None, &resp_with_data(json!({}))).unwrap();
            match receipt.side_effect_kind {
                SideEffectKind::Other { verb } => assert_eq!(verb, "Updated"),
                other => panic!("expected Other(Updated) for {slug}, got {other:?}"),
            }
        }
    }

    // ── read tools: no receipt ──

    #[test]
    fn classify_read_only_tool_emits_no_receipt() {
        // The whole point of receipts is to record side effects. Read
        // tools (LIST / FETCH / GET / SEARCH) don't qualify.
        let resp = resp_with_data(json!({ "items": [] }));
        assert!(classify("GMAIL_FETCH_EMAILS", None, &resp).is_none());
        assert!(classify("SLACK_LIST_CONVERSATIONS", None, &resp).is_none());
        assert!(classify("GOOGLECALENDAR_EVENTS_LIST", None, &resp).is_none());
        assert!(classify("LINEAR_GET_ISSUE", None, &resp).is_none());
        assert!(classify("NOTION_SEARCH", None, &resp).is_none());
    }

    #[test]
    fn classify_slug_without_recognised_verb_emits_no_receipt() {
        // `*_SUBSCRIBE_*`, `*_DIAGNOSE_*` and other esoteric verbs we
        // haven't catalogued return None — the user sees nothing
        // rather than a misleading "✅ Subscribed". When a real verb
        // shows up in production logs, add it to write_verb_from_slug.
        let resp = resp_with_data(json!({}));
        assert!(classify("FOO_SUBSCRIBE_BAR", None, &resp).is_none());
    }
}
