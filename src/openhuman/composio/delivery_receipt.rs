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
    // Communication
    if let Some(receipt) = classify_gmail_send_email(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_gmail_create_draft(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_slack_send_message(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_linkedin_send_message(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_twilio_send_message(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_discord_send_message(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_telegram_send_message(tool, arguments, response) {
        return Some(receipt);
    }
    // Calendar
    if let Some(receipt) = classify_googlecalendar_create_event(tool, arguments, response) {
        return Some(receipt);
    }
    // Files & docs
    if let Some(receipt) = classify_notion_create_page(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_notion_update_page(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_googledrive_upload_file(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_googledocs_create_doc(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_googlesheets_append_values(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_googlesheets_update_values(tool, arguments, response) {
        return Some(receipt);
    }
    // Issue trackers
    if let Some(receipt) = classify_linear_create_issue(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_github_create_issue(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_github_create_pull_request(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_jira_create_issue(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_asana_create_task(tool, arguments, response) {
        return Some(receipt);
    }
    // Social
    if let Some(receipt) = classify_linkedin_create_post(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_twitter_create_tweet(tool, arguments, response) {
        return Some(receipt);
    }
    // CRM / structured records
    if let Some(receipt) = classify_attio_create_record(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_airtable_create_record(tool, arguments, response) {
        return Some(receipt);
    }
    if let Some(receipt) = classify_hubspot_create_contact(tool, arguments, response) {
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
    let message_id =
        string_from_data(&resp.data, "id").or_else(|| message_id_from_response_id(resp));
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

// ── T-2b: extended curated rules (top write actions) ──────────────
//
// One fn per slug for readability. Each follows the same shape as the
// 6 original rules: match-or-return-None, extract recipient from args,
// extract message_id + link from response, emit `DeliveryReceipt`.
// Field lookups are best-effort — when Composio's response shape
// drifts the receipt still emits with `None` for the affected field
// rather than skipping (OQ-T1-A).

fn classify_gmail_create_draft(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "GMAIL_CREATE_DRAFT" && tool != "GMAIL_CREATE_EMAIL_DRAFT" {
        return None;
    }
    let recipient = string_from_args(args, "recipient_email").or_else(|| string_from_args(args, "to"));
    let message_id =
        string_from_data(&resp.data, "id").or_else(|| nested_string(&resp.data, &["draft", "id"]));
    let link = message_id
        .as_ref()
        .map(|id| format!("https://mail.google.com/mail/u/0/#drafts/{id}"));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::FileCreated {
            provider: "gmail".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_linkedin_send_message(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "LINKEDIN_SEND_MESSAGE" && tool != "LINKEDIN_CREATE_DIRECT_MESSAGE" {
        return None;
    }
    let recipient = string_from_args(args, "recipient_urn")
        .or_else(|| string_from_args(args, "recipient"))
        .or_else(|| string_from_args(args, "to"));
    let message_id = string_from_data(&resp.data, "id");
    // No reliable canonical deep-link template — surfaces with link: None.
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::MessagePosted {
            provider: "linkedin".to_string(),
        },
        recipient,
        message_id,
        link: None,
        at: Utc::now(),
    })
}

fn classify_twilio_send_message(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    // Twilio's canonical Composio slug is TWILIO_CREATE_MESSAGE; we
    // also accept SEND_SMS as a forward-compat alias for any future
    // slug rename.
    if tool != "TWILIO_CREATE_MESSAGE" && tool != "TWILIO_SEND_SMS" {
        return None;
    }
    let recipient = string_from_args(args, "to");
    // Twilio's response surfaces `sid` (e.g. `SMxxxxxxxx`).
    let message_id = string_from_data(&resp.data, "sid").or_else(|| string_from_data(&resp.data, "id"));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::MessagePosted {
            provider: "twilio".to_string(),
        },
        recipient,
        message_id,
        link: None,
        at: Utc::now(),
    })
}

fn classify_discord_send_message(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "DISCORD_SEND_MESSAGE" && tool != "DISCORD_POST_MESSAGE" {
        return None;
    }
    let recipient = string_from_args(args, "channel_id").or_else(|| string_from_args(args, "channel"));
    let message_id = string_from_data(&resp.data, "id");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::MessagePosted {
            provider: "discord".to_string(),
        },
        recipient,
        message_id,
        link: None,
        at: Utc::now(),
    })
}

fn classify_telegram_send_message(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "TELEGRAM_SEND_MESSAGE" && tool != "TELEGRAM_BOT_SEND_MESSAGE" {
        return None;
    }
    let recipient = string_from_args(args, "chat_id");
    // Telegram returns `message_id` as a number; coerce.
    let message_id = resp
        .data
        .get("message_id")
        .and_then(|v| v.as_i64().map(|n| n.to_string()).or_else(|| v.as_str().map(str::to_string)))
        .or_else(|| nested_string(&resp.data, &["result", "message_id"]));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::MessagePosted {
            provider: "telegram".to_string(),
        },
        recipient,
        message_id,
        link: None,
        at: Utc::now(),
    })
}

fn classify_notion_update_page(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "NOTION_UPDATE_PAGE"
        && tool != "NOTION_UPDATE_BLOCK"
        && tool != "NOTION_APPEND_BLOCK_CHILDREN"
    {
        return None;
    }
    let recipient = string_from_args(args, "page_id")
        .or_else(|| string_from_args(args, "block_id"))
        .or_else(|| string_from_args(args, "title"));
    let message_id = string_from_data(&resp.data, "id").or(recipient.clone());
    let link = string_from_data(&resp.data, "url");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::RecordUpdated {
            provider: "notion".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_googledrive_upload_file(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "GOOGLEDRIVE_UPLOAD_FILE"
        && tool != "GOOGLEDRIVE_CREATE_FILE"
        && tool != "GOOGLEDRIVE_CREATE_FOLDER"
    {
        return None;
    }
    let recipient = string_from_args(args, "file_name")
        .or_else(|| string_from_args(args, "name"))
        .or_else(|| string_from_args(args, "title"));
    let message_id = string_from_data(&resp.data, "id");
    let link = string_from_data(&resp.data, "webViewLink").or_else(|| {
        message_id
            .as_ref()
            .map(|id| format!("https://drive.google.com/file/d/{id}/view"))
    });
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::FileCreated {
            provider: "googledrive".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_googledocs_create_doc(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "GOOGLEDOCS_CREATE_DOC"
        && tool != "GOOGLEDOCS_CREATE_DOCUMENT"
        && tool != "GOOGLEDOCS_CREATE_DOCUMENT_FROM_TEXT"
    {
        return None;
    }
    let recipient = string_from_args(args, "title").or_else(|| string_from_args(args, "name"));
    let message_id =
        string_from_data(&resp.data, "documentId").or_else(|| string_from_data(&resp.data, "id"));
    let link = message_id
        .as_ref()
        .map(|id| format!("https://docs.google.com/document/d/{id}/edit"));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::FileCreated {
            provider: "googledocs".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_googlesheets_append_values(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "GOOGLESHEETS_APPEND_VALUES"
        && tool != "GOOGLESHEETS_SPREADSHEETS_VALUES_APPEND"
        && tool != "GOOGLESHEETS_BATCH_UPDATE_BY_DATA_FILTER"
    {
        return None;
    }
    // Args: spreadsheet_id (or spreadsheetId), range, values
    let spreadsheet_id = string_from_args(args, "spreadsheet_id")
        .or_else(|| string_from_args(args, "spreadsheetId"));
    let range = string_from_args(args, "range");
    let recipient = match (range.as_deref(), spreadsheet_id.as_deref()) {
        (Some(r), Some(_)) => Some(r.to_string()),
        (Some(r), None) => Some(r.to_string()),
        (None, Some(id)) => Some(id.to_string()),
        _ => None,
    };
    let message_id = string_from_data(&resp.data, "spreadsheetId").or(spreadsheet_id.clone());
    let link = message_id
        .as_ref()
        .map(|id| format!("https://docs.google.com/spreadsheets/d/{id}/edit"));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::RecordCreated {
            provider: "googlesheets".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_googlesheets_update_values(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "GOOGLESHEETS_UPDATE_SPREADSHEET_VALUES"
        && tool != "GOOGLESHEETS_BATCH_UPDATE"
        && tool != "GOOGLESHEETS_SPREADSHEETS_VALUES_UPDATE"
        && tool != "GOOGLESHEETS_SPREADSHEETS_BATCH_UPDATE"
    {
        return None;
    }
    let spreadsheet_id = string_from_args(args, "spreadsheet_id")
        .or_else(|| string_from_args(args, "spreadsheetId"));
    let range = string_from_args(args, "range")
        .or_else(|| string_from_data(&resp.data, "updatedRange"));
    let recipient = range.or_else(|| spreadsheet_id.clone());
    let message_id = string_from_data(&resp.data, "spreadsheetId").or(spreadsheet_id.clone());
    let link = message_id
        .as_ref()
        .map(|id| format!("https://docs.google.com/spreadsheets/d/{id}/edit"));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::RecordUpdated {
            provider: "googlesheets".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_linear_create_issue(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "LINEAR_CREATE_ISSUE" && tool != "LINEAR_CREATE_LINEAR_ISSUE" {
        return None;
    }
    let recipient = string_from_args(args, "title");
    // Linear's response wraps under `issue`: { id, identifier, url, … }.
    let message_id = nested_string(&resp.data, &["issue", "identifier"])
        .or_else(|| nested_string(&resp.data, &["issue", "id"]))
        .or_else(|| string_from_data(&resp.data, "id"));
    let link = nested_string(&resp.data, &["issue", "url"])
        .or_else(|| string_from_data(&resp.data, "url"));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::IssueCreated {
            provider: "linear".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_github_create_issue(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "GITHUB_CREATE_ISSUE" && tool != "GITHUB_ISSUES_CREATE" {
        return None;
    }
    let recipient = string_from_args(args, "title");
    // GitHub returns issue number + html_url directly.
    let message_id = resp
        .data
        .get("number")
        .and_then(|v| v.as_i64().map(|n| format!("#{n}")))
        .or_else(|| string_from_data(&resp.data, "id"));
    let link = string_from_data(&resp.data, "html_url");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::IssueCreated {
            provider: "github".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_github_create_pull_request(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "GITHUB_CREATE_PULL_REQUEST" && tool != "GITHUB_PULLS_CREATE" {
        return None;
    }
    let recipient = string_from_args(args, "title");
    let message_id = resp
        .data
        .get("number")
        .and_then(|v| v.as_i64().map(|n| format!("#{n}")))
        .or_else(|| string_from_data(&resp.data, "id"));
    let link = string_from_data(&resp.data, "html_url");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::IssueCreated {
            provider: "github".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_jira_create_issue(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "JIRA_CREATE_ISSUE" && tool != "JIRA_CREATE_JIRA_ISSUE" {
        return None;
    }
    let recipient = nested_string(args.unwrap_or(&Value::Null), &["fields", "summary"])
        .or_else(|| string_from_args(args, "summary"));
    let message_id = string_from_data(&resp.data, "key").or_else(|| string_from_data(&resp.data, "id"));
    let link = string_from_data(&resp.data, "self");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::IssueCreated {
            provider: "jira".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_asana_create_task(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "ASANA_CREATE_TASK" && tool != "ASANA_TASKS_CREATE_TASK" {
        return None;
    }
    let recipient = string_from_args(args, "name").or_else(|| string_from_args(args, "title"));
    let message_id = nested_string(&resp.data, &["data", "gid"])
        .or_else(|| string_from_data(&resp.data, "gid"))
        .or_else(|| string_from_data(&resp.data, "id"));
    let link = nested_string(&resp.data, &["data", "permalink_url"])
        .or_else(|| string_from_data(&resp.data, "permalink_url"));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::IssueCreated {
            provider: "asana".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_linkedin_create_post(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "LINKEDIN_CREATE_LINKED_IN_POST"
        && tool != "LINKEDIN_CREATE_POST"
        && tool != "LINKEDIN_SHARE_POST"
    {
        return None;
    }
    let recipient = string_from_args(args, "text")
        .or_else(|| string_from_args(args, "commentary"))
        .or_else(|| string_from_args(args, "content"));
    // Truncate the recipient text — posts are long, the receipt row
    // needs to stay tidy.
    let recipient = recipient.map(|r| truncate_for_display(&r, 60));
    let message_id = string_from_data(&resp.data, "id");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::SocialPostCreated {
            provider: "linkedin".to_string(),
        },
        recipient,
        message_id,
        link: None,
        at: Utc::now(),
    })
}

fn classify_twitter_create_tweet(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "TWITTER_POST_TWEET"
        && tool != "TWITTER_CREATION_OF_A_POST"
        && tool != "TWITTER_CREATE_TWEET"
        && tool != "TWITTER_TWEETS_POST"
    {
        return None;
    }
    let recipient = string_from_args(args, "text").map(|r| truncate_for_display(&r, 60));
    let message_id = string_from_data(&resp.data, "id")
        .or_else(|| nested_string(&resp.data, &["data", "id"]));
    // Tweet permalink template uses /i/web/status/{id} which redirects
    // to the canonical username URL.
    let link = message_id
        .as_ref()
        .map(|id| format!("https://twitter.com/i/web/status/{id}"));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::SocialPostCreated {
            provider: "twitter".to_string(),
        },
        recipient,
        message_id,
        link,
        at: Utc::now(),
    })
}

fn classify_airtable_create_record(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "AIRTABLE_CREATE_RECORD" && tool != "AIRTABLE_CREATE_RECORDS" {
        return None;
    }
    let recipient = string_from_args(args, "table_name")
        .or_else(|| string_from_args(args, "tableId"));
    let message_id = string_from_data(&resp.data, "id")
        .or_else(|| nested_string(&resp.data, &["records", "0", "id"]));
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::RecordCreated {
            provider: "airtable".to_string(),
        },
        recipient,
        message_id,
        link: None,
        at: Utc::now(),
    })
}

fn classify_hubspot_create_contact(
    tool: &str,
    args: Option<&Value>,
    resp: &ComposioExecuteResponse,
) -> Option<DeliveryReceipt> {
    if tool != "HUBSPOT_CREATE_CONTACT"
        && tool != "HUBSPOT_CRM_CONTACTS_CREATE"
        && tool != "HUBSPOT_CONTACTS_CREATE"
    {
        return None;
    }
    let recipient = nested_string(args.unwrap_or(&Value::Null), &["properties", "email"])
        .or_else(|| string_from_args(args, "email"))
        .or_else(|| nested_string(args.unwrap_or(&Value::Null), &["properties", "firstname"]));
    let message_id = string_from_data(&resp.data, "id");
    Some(DeliveryReceipt {
        tool: tool.to_string(),
        side_effect_kind: SideEffectKind::RecordCreated {
            provider: "hubspot".to_string(),
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

/// Walk a JSON path of keys, returning the leaf string if present.
/// Numeric keys (e.g. `"0"`) are interpreted as array indices.
/// Used by classifiers whose providers nest the relevant id /
/// link / title under multi-level structures (Linear, Jira, Asana,
/// Hubspot).
fn nested_string(root: &Value, path: &[&str]) -> Option<String> {
    let mut cursor = root;
    for segment in path {
        cursor = match cursor {
            Value::Object(map) => map.get(*segment)?,
            Value::Array(items) => {
                let idx: usize = segment.parse().ok()?;
                items.get(idx)?
            }
            _ => return None,
        };
    }
    cursor.as_str().map(str::to_string)
}

/// Cap a free-form text string at `max_chars` Unicode scalars so a
/// long recipient (LinkedIn post body, tweet text) doesn't blow out
/// the receipt row's layout. Adds a single-char ellipsis when
/// truncated.
fn truncate_for_display(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…")
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

    // ── T-2b: extended curated rules ───────────────────────────────

    // Communication

    #[test]
    fn classify_gmail_create_draft_emits_file_created_with_draft_link() {
        let args = json!({ "recipient_email": "x@y.com", "subject": "draft" });
        let resp = resp_with_data(json!({ "id": "draft_abc" }));
        let receipt = classify("GMAIL_CREATE_DRAFT", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::FileCreated { ref provider } if provider == "gmail"
        ));
        assert_eq!(
            receipt.link.as_deref(),
            Some("https://mail.google.com/mail/u/0/#drafts/draft_abc")
        );
        assert_eq!(receipt.recipient.as_deref(), Some("x@y.com"));
    }

    #[test]
    fn classify_linkedin_send_message_classifies_as_message_posted_linkedin() {
        let args = json!({ "recipient_urn": "urn:li:person:abc", "message": "hi" });
        let resp = resp_with_data(json!({ "id": "msg_xyz" }));
        let receipt = classify("LINKEDIN_SEND_MESSAGE", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::MessagePosted { ref provider } if provider == "linkedin"
        ));
        assert_eq!(receipt.recipient.as_deref(), Some("urn:li:person:abc"));
        assert_eq!(receipt.message_id.as_deref(), Some("msg_xyz"));
        assert!(receipt.link.is_none());
    }

    #[test]
    fn classify_twilio_create_message_extracts_sid_and_to() {
        let args = json!({ "to": "+15551234567", "body": "hello" });
        let resp = resp_with_data(json!({ "sid": "SMabc123" }));
        let receipt = classify("TWILIO_CREATE_MESSAGE", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::MessagePosted { ref provider } if provider == "twilio"
        ));
        assert_eq!(receipt.recipient.as_deref(), Some("+15551234567"));
        assert_eq!(receipt.message_id.as_deref(), Some("SMabc123"));
    }

    #[test]
    fn classify_discord_send_message_uses_channel_id() {
        let args = json!({ "channel_id": "1234567890", "content": "hi" });
        let resp = resp_with_data(json!({ "id": "msg_1" }));
        let receipt = classify("DISCORD_SEND_MESSAGE", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::MessagePosted { ref provider } if provider == "discord"
        ));
        assert_eq!(receipt.recipient.as_deref(), Some("1234567890"));
    }

    #[test]
    fn classify_telegram_send_message_extracts_numeric_message_id() {
        let args = json!({ "chat_id": "5555", "text": "hi" });
        let resp = resp_with_data(json!({ "message_id": 42 }));
        let receipt = classify("TELEGRAM_SEND_MESSAGE", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::MessagePosted { ref provider } if provider == "telegram"
        ));
        assert_eq!(receipt.message_id.as_deref(), Some("42"));
    }

    // Files & docs

    #[test]
    fn classify_notion_update_page_classifies_as_record_updated() {
        let args = json!({ "page_id": "p1" });
        let resp = resp_with_data(json!({ "id": "p1", "url": "https://notion.so/p1" }));
        let receipt = classify("NOTION_UPDATE_PAGE", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::RecordUpdated { ref provider } if provider == "notion"
        ));
        assert_eq!(receipt.link.as_deref(), Some("https://notion.so/p1"));
    }

    #[test]
    fn classify_googledrive_upload_file_extracts_webview_link() {
        let args = json!({ "file_name": "report.pdf" });
        let resp = resp_with_data(json!({
            "id": "drive_abc",
            "webViewLink": "https://drive.google.com/file/d/drive_abc/view"
        }));
        let receipt = classify("GOOGLEDRIVE_UPLOAD_FILE", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::FileCreated { ref provider } if provider == "googledrive"
        ));
        assert_eq!(receipt.recipient.as_deref(), Some("report.pdf"));
        assert_eq!(
            receipt.link.as_deref(),
            Some("https://drive.google.com/file/d/drive_abc/view")
        );
    }

    #[test]
    fn classify_googledocs_create_doc_builds_link_from_document_id() {
        let args = json!({ "title": "Meeting notes" });
        let resp = resp_with_data(json!({ "documentId": "doc_xyz" }));
        let receipt = classify("GOOGLEDOCS_CREATE_DOC", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::FileCreated { ref provider } if provider == "googledocs"
        ));
        assert_eq!(
            receipt.link.as_deref(),
            Some("https://docs.google.com/document/d/doc_xyz/edit")
        );
    }

    #[test]
    fn classify_googlesheets_append_values_record_created() {
        let args = json!({
            "spreadsheet_id": "sheet_abc",
            "range": "Sheet1!A1:C1",
            "values": [["a", "b", "c"]]
        });
        let resp = resp_with_data(json!({ "spreadsheetId": "sheet_abc" }));
        let receipt = classify("GOOGLESHEETS_APPEND_VALUES", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::RecordCreated { ref provider } if provider == "googlesheets"
        ));
        assert_eq!(receipt.recipient.as_deref(), Some("Sheet1!A1:C1"));
        assert_eq!(
            receipt.link.as_deref(),
            Some("https://docs.google.com/spreadsheets/d/sheet_abc/edit")
        );
    }

    #[test]
    fn classify_googlesheets_update_values_record_updated() {
        let args = json!({ "spreadsheet_id": "sheet_xyz", "range": "Sheet1!B2" });
        let resp = resp_with_data(json!({ "spreadsheetId": "sheet_xyz" }));
        let receipt =
            classify("GOOGLESHEETS_UPDATE_SPREADSHEET_VALUES", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::RecordUpdated { ref provider } if provider == "googlesheets"
        ));
        assert!(receipt
            .link
            .as_deref()
            .unwrap()
            .starts_with("https://docs.google.com/spreadsheets/d/sheet_xyz"));
    }

    // Issue trackers

    #[test]
    fn classify_linear_create_issue_extracts_nested_identifier_and_url() {
        let args = json!({ "title": "Fix the bug" });
        let resp = resp_with_data(json!({
            "issue": {
                "id": "uuid-abc",
                "identifier": "ENG-123",
                "url": "https://linear.app/acme/issue/ENG-123"
            }
        }));
        let receipt = classify("LINEAR_CREATE_ISSUE", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::IssueCreated { ref provider } if provider == "linear"
        ));
        assert_eq!(receipt.recipient.as_deref(), Some("Fix the bug"));
        assert_eq!(receipt.message_id.as_deref(), Some("ENG-123"));
        assert_eq!(
            receipt.link.as_deref(),
            Some("https://linear.app/acme/issue/ENG-123")
        );
    }

    #[test]
    fn classify_github_create_issue_formats_number_with_hash() {
        let args = json!({ "title": "Login broken" });
        let resp = resp_with_data(json!({
            "number": 42,
            "html_url": "https://github.com/owner/repo/issues/42"
        }));
        let receipt = classify("GITHUB_CREATE_ISSUE", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::IssueCreated { ref provider } if provider == "github"
        ));
        assert_eq!(receipt.message_id.as_deref(), Some("#42"));
        assert_eq!(
            receipt.link.as_deref(),
            Some("https://github.com/owner/repo/issues/42")
        );
    }

    #[test]
    fn classify_github_create_pull_request_classifies_as_issue_too() {
        // PRs visually fit "issue created" — different artifact type
        // but same affordance for the user: clickable link, identifier.
        let args = json!({ "title": "Refactor auth" });
        let resp = resp_with_data(json!({
            "number": 7,
            "html_url": "https://github.com/owner/repo/pull/7"
        }));
        let receipt = classify("GITHUB_CREATE_PULL_REQUEST", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::IssueCreated { ref provider } if provider == "github"
        ));
        assert!(receipt.link.as_deref().unwrap().contains("/pull/7"));
    }

    #[test]
    fn classify_jira_create_issue_extracts_key_from_response() {
        // Jira's response carries `key` (e.g. "PROJ-42") + `fields.summary`
        // in the request body.
        let args = json!({ "fields": { "summary": "Investigate latency" } });
        let resp = resp_with_data(json!({ "key": "PROJ-42", "id": "10042" }));
        let receipt = classify("JIRA_CREATE_ISSUE", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::IssueCreated { ref provider } if provider == "jira"
        ));
        assert_eq!(receipt.recipient.as_deref(), Some("Investigate latency"));
        assert_eq!(receipt.message_id.as_deref(), Some("PROJ-42"));
    }

    #[test]
    fn classify_asana_create_task_extracts_nested_gid_and_permalink() {
        let args = json!({ "name": "Write the doc" });
        let resp = resp_with_data(json!({
            "data": {
                "gid": "asana_123",
                "permalink_url": "https://app.asana.com/0/0/asana_123"
            }
        }));
        let receipt = classify("ASANA_CREATE_TASK", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::IssueCreated { ref provider } if provider == "asana"
        ));
        assert_eq!(receipt.message_id.as_deref(), Some("asana_123"));
        assert_eq!(
            receipt.link.as_deref(),
            Some("https://app.asana.com/0/0/asana_123")
        );
    }

    // Social

    #[test]
    fn classify_linkedin_create_post_classifies_as_social_post() {
        let args = json!({
            "commentary": "Excited to announce we shipped Trust UX in OpenHuman 🎉 Lots more to come!"
        });
        let resp = resp_with_data(json!({ "id": "urn:li:share:abc" }));
        let receipt = classify("LINKEDIN_CREATE_LINKED_IN_POST", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::SocialPostCreated { ref provider } if provider == "linkedin"
        ));
        assert!(
            receipt.recipient.as_deref().unwrap().ends_with('…'),
            "long post bodies must truncate; got: {:?}",
            receipt.recipient
        );
    }

    #[test]
    fn classify_twitter_post_tweet_builds_permalink_from_id() {
        let args = json!({ "text": "hello world" });
        let resp = resp_with_data(json!({ "data": { "id": "1799999999999999999" } }));
        let receipt = classify("TWITTER_POST_TWEET", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::SocialPostCreated { ref provider } if provider == "twitter"
        ));
        assert_eq!(
            receipt.link.as_deref(),
            Some("https://twitter.com/i/web/status/1799999999999999999")
        );
    }

    // CRM / structured records

    #[test]
    fn classify_airtable_create_record_classifies_as_record_created() {
        let args = json!({ "table_name": "Leads" });
        let resp = resp_with_data(json!({ "id": "rec_abc" }));
        let receipt = classify("AIRTABLE_CREATE_RECORD", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::RecordCreated { ref provider } if provider == "airtable"
        ));
        assert_eq!(receipt.recipient.as_deref(), Some("Leads"));
    }

    #[test]
    fn classify_hubspot_create_contact_extracts_nested_email() {
        let args = json!({
            "properties": { "email": "lead@example.com", "firstname": "Alex" }
        });
        let resp = resp_with_data(json!({ "id": "contact_42" }));
        let receipt = classify("HUBSPOT_CREATE_CONTACT", Some(&args), &resp).unwrap();
        assert!(matches!(
            receipt.side_effect_kind,
            SideEffectKind::RecordCreated { ref provider } if provider == "hubspot"
        ));
        assert_eq!(receipt.recipient.as_deref(), Some("lead@example.com"));
        assert_eq!(receipt.message_id.as_deref(), Some("contact_42"));
    }
}
