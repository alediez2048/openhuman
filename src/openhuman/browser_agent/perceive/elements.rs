//! F3-2 — element + state shapes used inside a [`super::PageSnapshot`].

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::openhuman::browser_agent::cdp::types::Rect;

/// One element the agent can interact with. `id` is sequential and
/// stable WITHIN a single snapshot — the LLM uses it as a numeric
/// handle ("click [3]"). Across snapshots, ids reset; the durable
/// re-find handle is [`Self::xpath`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionableElement {
    pub id: u32,
    pub role: ElementRole,
    /// Accessible name. Computed as:
    /// `aria-label || textContent.trim() || placeholder || value || ""`.
    pub label: String,
    pub state: ElementState,
    pub bounds: Rect,
    pub xpath: String,
    /// Narrow whitelist of useful attributes. Bigger sets bloat
    /// snapshot tokens without much agent value — `href`, `value`,
    /// `placeholder`, `name`, `type`, and any `aria-*` are typical.
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

/// Coarse-grained role classification. Maps from the DOM tag + ARIA
/// role hybrid into a small enum the LLM can reason about cleanly.
/// Unknown shapes degrade to [`ElementRole::Other`] with the source
/// string preserved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ElementRole {
    Button,
    Link,
    /// `<input>` of any type — `text`, `email`, `search`, `password`,
    /// etc. The specific type lives in
    /// `attributes["type"]` so the LLM can disambiguate.
    Input,
    Textarea,
    Select,
    Checkbox,
    Radio,
    Heading,
    Image,
    /// Static text region pulled in because it sits near an actionable
    /// element (e.g. a label next to a button). Helps the LLM tell
    /// "the Save button" from "the Delete button".
    Text,
    /// Iframe — Phase 3.1 doesn't recurse, but flags the boundary.
    IframePresent {
        src: String,
    },
    /// Anything that matched the actionable filter but didn't classify
    /// cleanly. Surfaces the raw tag for the LLM to reason about.
    Other {
        tag: String,
    },
}

impl ElementRole {
    /// Short label used by [`super::render::to_llm_text`] — e.g.
    /// "button", "input", "link".
    pub fn render_label(&self) -> String {
        match self {
            ElementRole::Button => "button".into(),
            ElementRole::Link => "link".into(),
            ElementRole::Input => "input".into(),
            ElementRole::Textarea => "textarea".into(),
            ElementRole::Select => "select".into(),
            ElementRole::Checkbox => "checkbox".into(),
            ElementRole::Radio => "radio".into(),
            ElementRole::Heading => "heading".into(),
            ElementRole::Image => "image".into(),
            ElementRole::Text => "text".into(),
            ElementRole::IframePresent { .. } => "iframe".into(),
            ElementRole::Other { tag } => tag.clone(),
        }
    }
}

/// Element interaction state. The agent reasons about these to pick
/// which element makes sense to act on.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ElementState {
    pub disabled: bool,
    pub checked: bool,
    pub expanded: bool,
    pub focused: bool,
    pub hidden: bool,
}

/// Page viewport in CSS pixels, top-left origin. Stored on every
/// snapshot so F3-7 vision fallback can map between snapshot coords
/// and screenshot coords.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
    pub device_pixel_ratio: f64,
}
