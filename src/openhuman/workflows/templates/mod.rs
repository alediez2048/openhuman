//! Bundled starter-template catalog (F-5).
//!
//! Four JSON files (RU-1..RU-4) ship in-binary via `include_str!` per
//! ADR-004. `all_bundled()` parses them into [`StarterTemplate`] at
//! every call; the cost is bounded (4 small files) and parsing eagerly
//! means a malformed template surfaces in the templates_tests build
//! before reaching production.
//!
//! See `Automations/Tickets/phase-1-foundation/F-5.md` for the
//! authoring spec and `Automations/ADRs/ADR-004-templates-shipped-as-in-repo-json.md`
//! / `ADR-008-starter-templates-as-readonly-catalog.md` for the
//! design rationale.

use crate::openhuman::workflows::types::StarterTemplate;

pub const RU_1_JSON: &str = include_str!("ru-1-founder-morning-digest.json");
pub const RU_2_JSON: &str = include_str!("ru-2-linkedin-engagement-queue.json");
pub const RU_3_JSON: &str = include_str!("ru-3-spotify-friday-five.json");
pub const RU_4_JSON: &str = include_str!("ru-4-jira-sprint-retro.json");

/// Ordered list of every bundled template as a `(label, json)` pair.
/// Used by tests so the failure message names which file is broken,
/// not just the parse error.
pub const BUNDLED_JSON: &[(&str, &str)] = &[
    ("ru-1-founder-morning-digest", RU_1_JSON),
    ("ru-2-linkedin-engagement-queue", RU_2_JSON),
    ("ru-3-spotify-friday-five", RU_3_JSON),
    ("ru-4-jira-sprint-retro", RU_4_JSON),
];

/// Parse every bundled template into a typed [`StarterTemplate`].
/// Malformed templates are logged and skipped so a broken file in the
/// bundle doesn't take the whole catalog down — the
/// `templates_tests::every_bundled_template_parses` test catches the
/// bad file at build time before it ever ships.
pub fn all_bundled() -> Vec<StarterTemplate> {
    BUNDLED_JSON
        .iter()
        .filter_map(
            |(label, raw)| match serde_json::from_str::<StarterTemplate>(raw) {
                Ok(t) => Some(t),
                Err(err) => {
                    tracing::error!(
                        target: "workflows",
                        "[workflows-templates] failed to parse `{label}`: {err}"
                    );
                    None
                }
            },
        )
        .collect()
}

/// Raw JSON for a single template by `template_id`. Used by the
/// catalog's [Add] flow to preserve the full body for
/// `workflows_create` without a re-serialize round-trip.
pub fn raw_payload_for(template_id: &str) -> Option<serde_json::Value> {
    BUNDLED_JSON
        .iter()
        .find_map(|(label, raw)| (*label == template_id).then(|| serde_json::from_str(raw).ok()))
        .flatten()
}
