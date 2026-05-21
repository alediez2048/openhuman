//! F-5 — per-template parse + structural tests for the four bundled
//! RU-* JSON files.
//!
//! These run at build time so any malformed template is caught before
//! it ships. The crons are validated through the same `cron` crate
//! parser F-11's validator uses.

use super::templates::{all_bundled, BUNDLED_JSON};
use crate::openhuman::workflows::types::{StarterTemplate, Trigger};
use std::collections::HashSet;
use std::str::FromStr;

#[test]
fn every_bundled_template_parses_cleanly() {
    // `all_bundled` only logs + skips bad files in production; here we
    // verify every individual file parses so a regression surfaces in
    // CI rather than silently dropping a template.
    for (label, raw) in BUNDLED_JSON {
        let parsed: Result<StarterTemplate, _> = serde_json::from_str(raw);
        assert!(
            parsed.is_ok(),
            "template `{label}` failed to parse: {err:#?}",
            err = parsed.err()
        );
    }
    assert_eq!(all_bundled().len(), BUNDLED_JSON.len());
}

#[test]
fn every_template_has_a_parseable_cron_expression() {
    // Templates use standard 5-field crontab syntax. The `cron` crate
    // itself requires a 6/7-field expression (Quartz-style with
    // seconds), so we route through `cron::schedule::normalize_expression`
    // — the same normalizer the production scheduler uses — before
    // handing the expression to `cron::Schedule::from_str`. F-11's
    // validator will pin this same path.
    use crate::openhuman::cron::normalize_expression;
    for t in all_bundled() {
        let trigger: Trigger = serde_json::from_value(t.trigger.clone()).unwrap_or_else(|err| {
            panic!(
                "template `{}` trigger failed to deserialize: {err}",
                t.template_id
            )
        });
        if let Trigger::Cron { ref expr, .. } = trigger {
            let normalized = normalize_expression(expr).unwrap_or_else(|err| {
                panic!(
                    "template `{}` cron `{expr}` could not be normalized: {err}",
                    t.template_id
                )
            });
            cron::Schedule::from_str(&normalized).unwrap_or_else(|err| {
                panic!(
                    "template `{}` cron `{expr}` (normalized to `{normalized}`) rejected by cron::Schedule::from_str: {err}",
                    t.template_id
                )
            });
        }
    }
}

#[test]
fn every_template_has_non_empty_required_connections() {
    for t in all_bundled() {
        assert!(
            !t.required_connections.is_empty(),
            "template `{}` must declare at least one required connection",
            t.template_id
        );
    }
}

#[test]
fn template_ids_are_unique() {
    let mut seen = HashSet::new();
    for t in all_bundled() {
        assert!(
            seen.insert(t.template_id.clone()),
            "duplicate template_id `{}` — every bundled template must declare a unique id",
            t.template_id
        );
    }
}

#[test]
fn ru_1_template_id_matches_the_e2e_spec() {
    // F-15's catalog E2E spec keys on this exact string. Pin it so a
    // typo on rename never breaks the contract.
    let ids: Vec<_> = all_bundled().into_iter().map(|t| t.template_id).collect();
    assert!(
        ids.contains(&"ru-1-founder-morning-digest".to_string()),
        "RU-1 must keep its locked template_id; saw {ids:?}"
    );
}

#[test]
fn every_template_declares_min_phase_one_or_higher() {
    for t in all_bundled() {
        assert!(
            t.min_phase >= 1,
            "template `{}` declares min_phase={}, must be ≥ 1",
            t.template_id,
            t.min_phase
        );
    }
}
