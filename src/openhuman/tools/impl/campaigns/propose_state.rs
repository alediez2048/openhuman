//! Shared "fetch campaign → validate proposed transition → render
//! `<campaign-preview>` tag" body for the three state-proposal tools
//! (`campaign_propose_pause` / `_resume` / `_archive`).
//!
//! Each propose-state tool delegates here so the three caller files
//! stay tiny and the preview-rendering contract has a single source
//! of truth.

use crate::openhuman::campaigns::ops;
use crate::openhuman::campaigns::types::{Campaign, CampaignStatus};
use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::ToolResult;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde_json::{json, Value};

/// Run the state-proposal flow for `action` on the campaign at
/// `args.id`. Builds the structured payload + the
/// `<campaign-preview>` tag the chat-runtime renders as an
/// Apply/Discard card. Validates the transition is legal before
/// proposing — illegal transitions return a structured error payload
/// so the agent can surface a clear message ("you can't pause an
/// already-Paused campaign") rather than silently round-trip the user.
///
/// `to` is the destination status the action implies (Paused for
/// pause, Active for resume, Archived for archive).
pub(super) async fn execute(
    config: &Config,
    args: Value,
    action: &'static str,
    to: CampaignStatus,
    fix_hint_for_illegal: fn(&Campaign) -> String,
) -> anyhow::Result<ToolResult> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("missing required field `id`"))?;

    let outcome = match ops::get(config, id.clone()).await {
        Ok(o) => o,
        Err(err) => {
            return Ok(ToolResult::error(format!(
                "campaign_propose_{action} failed: {err}"
            )))
        }
    };
    let Some(campaign) = outcome.value else {
        let payload = json!({
            "error": "not_found",
            "campaign_id": id,
        });
        return Ok(ToolResult::success(serde_json::to_string(&payload)?));
    };

    if !campaign.status.can_transition_to(to) {
        // Surface as success-with-structured-error so the agent can
        // tell the user what's wrong without a separate failure shape.
        let payload = json!({
            "error": "invalid_transition",
            "campaign_id": campaign.id,
            "from": campaign.status,
            "to": to,
            "hint": fix_hint_for_illegal(&campaign),
        });
        return Ok(ToolResult::success(serde_json::to_string(&payload)?));
    }

    let preview_payload = json!({
        "campaign_id": campaign.id,
        "name": campaign.name,
        "action": action,
        "from": campaign.status,
        "to": to,
    });
    let data_b64 = BASE64.encode(serde_json::to_string(&preview_payload)?.as_bytes());
    let preview_tag = format!(
        "<campaign-preview kind=\"state\" action=\"{action}\" data=\"{data_b64}\"></campaign-preview>"
    );

    let payload = json!({
        "status": "preview_ready",
        "render_instructions": format!(
            "Include the `preview_tag` value verbatim in your user-facing reply. \
             The user clicks Apply on the card to commit via campaigns_{action}. \
             Do not call campaign_propose_{action} again."
        ),
        "preview_tag": preview_tag,
        "preview": preview_payload,
    });
    let markdown = format!(
        "Proposing to {action} `{name}`. Include this tag verbatim in your reply:\n\n{preview_tag}",
        action = action,
        name = campaign.name,
    );
    Ok(ToolResult::success_with_markdown(payload, markdown))
}
