//! F3-2 — LLM-facing renderer.
//!
//! Turns a [`super::PageSnapshot`] into the `[N] role "label" — attrs`
//! text the workflow_node sub-agent reads. Three detail tiers
//! (Compact / Standard / Verbose) so F3-3 can budget tokens.

use super::elements::{ActionableElement, ElementRole};
use super::snapshot::PageSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTier {
    /// Top 30 actionable elements + page title + URL only.
    /// Used when the snapshot must fit comfortably (<500 tokens).
    Compact,
    /// Every actionable element + short text excerpt. **Default tier.**
    Standard,
    /// Adds attributes per element + larger text excerpt. Used when
    /// the LLM explicitly asks for more detail.
    Verbose,
}

pub fn to_llm_text(snap: &PageSnapshot, tier: DetailTier) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str(&format!("URL: {}\n", snap.url));
    if !snap.title.is_empty() {
        out.push_str(&format!("Title: {}\n", snap.title));
    }
    out.push('\n');

    let elements: Box<dyn Iterator<Item = &ActionableElement>> = match tier {
        DetailTier::Compact => Box::new(snap.elements.iter().take(30)),
        DetailTier::Standard | DetailTier::Verbose => Box::new(snap.elements.iter()),
    };

    for el in elements {
        render_one(&mut out, el, tier);
    }

    // Text excerpt — skipped in Compact (too dear); trimmed in
    // Standard; full in Verbose.
    let text_budget = match tier {
        DetailTier::Compact => 0,
        DetailTier::Standard => 400,
        DetailTier::Verbose => 1500,
    };
    if text_budget > 0 && !snap.text_content.is_empty() {
        let take = snap.text_content.chars().take(text_budget).collect::<String>();
        out.push_str("\n--- page text ---\n");
        out.push_str(&take);
        if snap.text_content.len() > take.len() {
            out.push_str("\n…(truncated)");
        }
    }

    out
}

fn render_one(out: &mut String, el: &ActionableElement, tier: DetailTier) {
    let role = el.role.render_label();
    let label = if el.label.is_empty() {
        "(no label)".to_string()
    } else {
        let trimmed: String = el.label.chars().take(80).collect();
        format!("\"{trimmed}\"")
    };

    // State suffix — disabled / checked / focused are the ones the
    // LLM is most likely to care about.
    let mut state_bits: Vec<&'static str> = Vec::new();
    if el.state.disabled {
        state_bits.push("disabled");
    }
    if el.state.checked {
        state_bits.push("checked");
    }
    if el.state.focused {
        state_bits.push("focused");
    }
    let state_suffix = if state_bits.is_empty() {
        String::new()
    } else {
        format!(" ({})", state_bits.join(", "))
    };

    out.push_str(&format!("[{}] {role} {label}{state_suffix}", el.id));

    // Attribute trail — href for links, type for inputs, etc.
    let attrs_summary = match (&el.role, tier) {
        (ElementRole::Link, _) => el.attributes.get("href").map(|h| format!(" → {}", trim(h, 60))),
        (ElementRole::Input, tier) => {
            let t = el.attributes.get("type").cloned().unwrap_or_default();
            let placeholder = el.attributes.get("placeholder").cloned().unwrap_or_default();
            let mut parts = Vec::new();
            if !t.is_empty() {
                parts.push(format!("type={t}"));
            }
            if !placeholder.is_empty() && tier != DetailTier::Compact {
                parts.push(format!("placeholder=\"{}\"", trim(&placeholder, 40)));
            }
            if parts.is_empty() {
                None
            } else {
                Some(format!(" [{}]", parts.join(", ")))
            }
        }
        (ElementRole::IframePresent { .. }, _) => {
            el.attributes.get("src").map(|s| format!(" src={}", trim(s, 60)))
        }
        _ => None,
    };
    if let Some(s) = attrs_summary {
        out.push_str(&s);
    }

    // Verbose tier dumps every attribute.
    if tier == DetailTier::Verbose && !el.attributes.is_empty() {
        let mut kv: Vec<String> = el
            .attributes
            .iter()
            .filter(|(k, _)| {
                // Already rendered as the "primary" trail above; skip.
                !matches!(k.as_str(), "href" | "type" | "placeholder" | "src")
            })
            .map(|(k, v)| format!("{k}={}", trim(v, 40)))
            .collect();
        if !kv.is_empty() {
            kv.sort();
            out.push_str(&format!(" {{ {} }}", kv.join(", ")));
        }
    }

    out.push('\n');
}

fn trim(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('…');
        out
    }
}
