//! F3-2 — snapshot entry point.
//!
//! `snapshot(session, opts)` issues a single `Runtime.evaluate` of
//! `dom_extractor.js` (bundled via `include_str!`), parses the JSON
//! return value into a [`PageSnapshot`], and pre-computes the LLM
//! token estimate so the F3-3 tool layer can budget without
//! re-parsing.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::elements::{ActionableElement, ElementRole, ElementState, Viewport};
use crate::openhuman::browser_agent::cdp::errors::CdpError;
use crate::openhuman::browser_agent::cdp::session::CdpSession;
use crate::openhuman::browser_agent::cdp::types::Rect;

pub const DOM_EXTRACTOR_JS: &str = include_str!("dom_extractor.js");

#[derive(Debug, Clone, Default)]
pub struct SnapshotOptions {
    /// When true, calls `Accessibility.getFullAXTree` and overlays
    /// role / label refinements on top of the DOM extractor's output.
    /// Phase 3.1 ships with this `false` by default — DOM-only is
    /// sufficient for ~90% of pages per the F3-2 ticket. Set true
    /// when DOM grounding alone returns too few actionable elements.
    pub include_accessibility_tree: bool,
}

/// One page's structured representation. Stable enough to round-trip
/// through serde for tests + the F3-5 live preview's debug surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageSnapshot {
    pub url: String,
    pub title: String,
    pub viewport: Viewport,
    pub elements: Vec<ActionableElement>,
    /// Trimmed `<main>`-or-`<body>` text. ≤ 1500 chars from the
    /// extractor; the renderer trims further per detail tier.
    pub text_content: String,
    pub timestamp: DateTime<Utc>,
    /// Pre-computed `chars/4` heuristic over the rendered standard-
    /// tier output. Used by F3-3 to pick a tier without re-rendering.
    pub snapshot_token_estimate: usize,
}

pub async fn snapshot(
    session: &CdpSession,
    opts: SnapshotOptions,
) -> Result<PageSnapshot, CdpError> {
    let raw = session.evaluate(DOM_EXTRACTOR_JS).await?;
    let mut snap = parse_dom_extractor_output(&raw)?;

    if opts.include_accessibility_tree {
        // F3-2 follow-up. The Accessibility.getFullAXTree call would
        // walk the returned tree, find DOM nodes whose `role` is a
        // generic guess from the extractor ("div" via `[role]`
        // selector), and overwrite with the WAI-ARIA-promoted role.
        // Documented here so the next ticket knows where to hook in.
        tracing::debug!(
            target: "browser-agent-perceive",
            "[snapshot] accessibility-tree augmentation requested but not yet implemented (F3-2 follow-up)"
        );
    }

    // Pre-compute the standard-tier token estimate so F3-3 can branch
    // on it cheaply.
    let standard_render = super::render::to_llm_text(&snap, super::render::DetailTier::Standard);
    snap.snapshot_token_estimate = estimate_tokens(&standard_render);
    Ok(snap)
}

/// `chars/4` heuristic — matches the F3-2 ticket's note. Good enough
/// for budget enforcement (the F3-3 tier picker just needs an order
/// of magnitude).
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / 4
}

/// Parses the JSON the DOM extractor returns into a typed
/// `PageSnapshot`. Pub(crate) so the tests can poke at the shape.
pub(crate) fn parse_dom_extractor_output(
    raw: &serde_json::Value,
) -> Result<PageSnapshot, CdpError> {
    let url = raw
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CdpError::Other("snapshot: missing `url`".into()))?
        .to_string();
    let title = raw
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let viewport = raw
        .get("viewport")
        .map(|v| Viewport {
            width: v.get("width").and_then(|x| x.as_f64()).unwrap_or(0.0),
            height: v.get("height").and_then(|x| x.as_f64()).unwrap_or(0.0),
            device_pixel_ratio: v
                .get("device_pixel_ratio")
                .and_then(|x| x.as_f64())
                .unwrap_or(1.0),
        })
        .unwrap_or_default();
    let text_content = raw
        .get("text_excerpt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let raw_elements = raw
        .get("elements")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut elements = Vec::with_capacity(raw_elements.len());
    for (idx, e) in raw_elements.iter().enumerate() {
        elements.push(parse_one_element(idx as u32 + 1, e));
    }

    Ok(PageSnapshot {
        url,
        title,
        viewport,
        elements,
        text_content,
        timestamp: Utc::now(),
        snapshot_token_estimate: 0,
    })
}

fn parse_one_element(id: u32, raw: &serde_json::Value) -> ActionableElement {
    let tag = raw
        .get("tag")
        .and_then(|v| v.as_str())
        .unwrap_or("div")
        .to_string();
    let role_hint = raw.get("role_hint").and_then(|v| v.as_str());
    let attrs_obj = raw.get("attrs").and_then(|v| v.as_object());
    let attr_type = attrs_obj
        .and_then(|o| o.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let role = classify_role(&tag, role_hint, attr_type);
    let bounds = raw
        .get("bounds")
        .map(|b| Rect {
            x: b.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
            y: b.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
            width: b.get("width").and_then(|v| v.as_f64()).unwrap_or(0.0),
            height: b.get("height").and_then(|v| v.as_f64()).unwrap_or(0.0),
        })
        .unwrap_or_default();

    let mut attributes = HashMap::new();
    if let Some(obj) = attrs_obj {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                attributes.insert(k.clone(), s.to_string());
            }
        }
    }

    ActionableElement {
        id,
        role,
        label: raw
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        state: ElementState {
            disabled: raw
                .get("disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            checked: raw
                .get("checked")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            expanded: raw
                .get("expanded")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            focused: raw
                .get("focused")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            hidden: raw.get("hidden").and_then(|v| v.as_bool()).unwrap_or(false),
        },
        bounds,
        xpath: raw
            .get("xpath")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        attributes,
    }
}

fn classify_role(tag: &str, role_hint: Option<&str>, input_type: &str) -> ElementRole {
    // Honour explicit ARIA role first.
    if let Some(role) = role_hint {
        match role {
            "button" => return ElementRole::Button,
            "link" => return ElementRole::Link,
            "checkbox" => return ElementRole::Checkbox,
            "radio" => return ElementRole::Radio,
            "textbox" => return ElementRole::Input,
            "heading" => return ElementRole::Heading,
            _ => {}
        }
    }
    match tag {
        "button" => ElementRole::Button,
        "a" => ElementRole::Link,
        "select" => ElementRole::Select,
        "textarea" => ElementRole::Textarea,
        "h1" | "h2" | "h3" => ElementRole::Heading,
        "img" => ElementRole::Image,
        "iframe" => ElementRole::IframePresent { src: String::new() },
        "input" => match input_type {
            "checkbox" => ElementRole::Checkbox,
            "radio" => ElementRole::Radio,
            _ => ElementRole::Input,
        },
        other => ElementRole::Other {
            tag: other.to_string(),
        },
    }
}
