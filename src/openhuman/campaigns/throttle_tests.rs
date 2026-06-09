//! F4-8 throttle gate tests. Exercises reserve / release / current
//! against an ephemeral SQLite workspace + verifies bucket-boundary
//! math.

use chrono::{DateTime, TimeZone, Utc};
use tempfile::TempDir;

use super::throttle::{current_window_start, window_duration, ThrottleGate};
use super::types::{Throttle, ThrottleWindow};
use crate::openhuman::config::Config;

fn fresh_workspace() -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, config)
}

// ── current_window_start ───────────────────────────────────────────

#[test]
fn current_window_start_per_day_aligns_to_midnight_utc() {
    let now: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 6, 9, 14, 32, 17).unwrap();
    let start = current_window_start(ThrottleWindow::PerDay, now);
    assert_eq!(start, Utc.with_ymd_and_hms(2026, 6, 9, 0, 0, 0).unwrap());
}

#[test]
fn current_window_start_per_hour_aligns_to_top_of_hour() {
    let now: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 6, 9, 14, 32, 17).unwrap();
    let start = current_window_start(ThrottleWindow::PerHour, now);
    assert_eq!(start, Utc.with_ymd_and_hms(2026, 6, 9, 14, 0, 0).unwrap());
}

#[test]
fn current_window_start_per_minute_aligns_to_top_of_minute() {
    let now: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 6, 9, 14, 32, 17).unwrap();
    let start = current_window_start(ThrottleWindow::PerMinute, now);
    assert_eq!(start, Utc.with_ymd_and_hms(2026, 6, 9, 14, 32, 0).unwrap());
}

#[test]
fn window_duration_matches_each_variant() {
    assert_eq!(
        window_duration(ThrottleWindow::PerDay),
        chrono::Duration::days(1)
    );
    assert_eq!(
        window_duration(ThrottleWindow::PerHour),
        chrono::Duration::hours(1)
    );
    assert_eq!(
        window_duration(ThrottleWindow::PerMinute),
        chrono::Duration::minutes(1)
    );
}

// ── reserve / release ─────────────────────────────────────────────

#[test]
fn reserve_returns_n_when_budget_has_room() {
    let (_dir, config) = fresh_workspace();
    let throttle = Throttle {
        max_per_window: 10,
        window: ThrottleWindow::PerDay,
    };
    let granted = ThrottleGate::reserve(&config, &"camp_1".into(), Some(&throttle), 3).unwrap();
    assert_eq!(granted, 3);
}

#[test]
fn reserve_consumes_until_budget_exhausted_then_returns_zero() {
    let (_dir, config) = fresh_workspace();
    let throttle = Throttle {
        max_per_window: 3,
        window: ThrottleWindow::PerDay,
    };
    let cid: String = "camp_2".into();
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 1).unwrap(), 1);
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 1).unwrap(), 1);
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 1).unwrap(), 1);
    // Next call must report 0 — no slot.
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 1).unwrap(), 0);
}

#[test]
fn reserve_partial_grant_when_request_exceeds_remaining() {
    let (_dir, config) = fresh_workspace();
    let throttle = Throttle {
        max_per_window: 5,
        window: ThrottleWindow::PerDay,
    };
    let cid: String = "camp_3".into();
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 3).unwrap(), 3);
    // 2 slots remain; asking for 5 yields 2.
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 5).unwrap(), 2);
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 1).unwrap(), 0);
}

#[test]
fn release_refunds_to_the_active_window() {
    let (_dir, config) = fresh_workspace();
    let throttle = Throttle {
        max_per_window: 2,
        window: ThrottleWindow::PerDay,
    };
    let cid: String = "camp_4".into();
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 2).unwrap(), 2);
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 1).unwrap(), 0);
    ThrottleGate::release(&config, &cid, Some(&throttle), 1).unwrap();
    // 1 slot freed → reserve(1) succeeds again.
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 1).unwrap(), 1);
}

#[test]
fn release_clamps_at_zero_when_caller_over_releases() {
    let (_dir, config) = fresh_workspace();
    let throttle = Throttle {
        max_per_window: 3,
        window: ThrottleWindow::PerDay,
    };
    let cid: String = "camp_5".into();
    assert_eq!(ThrottleGate::reserve(&config, &cid, Some(&throttle), 1).unwrap(), 1);
    // Over-release shouldn't crash — clamp to 0.
    ThrottleGate::release(&config, &cid, Some(&throttle), 99).unwrap();
    let snap = ThrottleGate::current(&config, &cid, Some(&throttle))
        .unwrap()
        .unwrap();
    assert_eq!(snap.consumed, 0);
}

#[test]
fn reserve_with_no_throttle_is_a_no_op_returning_n() {
    let (_dir, config) = fresh_workspace();
    let granted = ThrottleGate::reserve(&config, &"camp_6".into(), None, 7).unwrap();
    assert_eq!(granted, 7, "no throttle = no gate; caller proceeds");
}

#[test]
fn release_with_no_throttle_is_a_no_op() {
    let (_dir, config) = fresh_workspace();
    ThrottleGate::release(&config, &"camp_7".into(), None, 5).unwrap();
}

#[test]
fn current_returns_none_when_no_throttle_configured() {
    let (_dir, config) = fresh_workspace();
    let out = ThrottleGate::current(&config, &"camp_8".into(), None).unwrap();
    assert!(out.is_none());
}

#[test]
fn current_reflects_reserved_consumption_and_remaining() {
    let (_dir, config) = fresh_workspace();
    let throttle = Throttle {
        max_per_window: 10,
        window: ThrottleWindow::PerDay,
    };
    let cid: String = "camp_9".into();
    ThrottleGate::reserve(&config, &cid, Some(&throttle), 4).unwrap();
    let snap = ThrottleGate::current(&config, &cid, Some(&throttle))
        .unwrap()
        .unwrap();
    assert_eq!(snap.consumed, 4);
    assert_eq!(snap.remaining, 6);
    assert_eq!(snap.limit, 10);
    assert!(matches!(snap.window, ThrottleWindow::PerDay));
    // next_window_at is exactly 1 day after window_start.
    assert_eq!(
        snap.next_window_at - snap.window_start,
        chrono::Duration::days(1)
    );
}

#[test]
fn buckets_are_per_campaign_so_two_campaigns_share_no_budget() {
    let (_dir, config) = fresh_workspace();
    let throttle = Throttle {
        max_per_window: 1,
        window: ThrottleWindow::PerDay,
    };
    assert_eq!(
        ThrottleGate::reserve(&config, &"camp_a".into(), Some(&throttle), 1).unwrap(),
        1
    );
    // camp_b has its own window row — also gets 1.
    assert_eq!(
        ThrottleGate::reserve(&config, &"camp_b".into(), Some(&throttle), 1).unwrap(),
        1
    );
    // Both are now full.
    assert_eq!(
        ThrottleGate::reserve(&config, &"camp_a".into(), Some(&throttle), 1).unwrap(),
        0
    );
    assert_eq!(
        ThrottleGate::reserve(&config, &"camp_b".into(), Some(&throttle), 1).unwrap(),
        0
    );
}

#[test]
fn reserve_zero_returns_zero_without_touching_the_store() {
    let (_dir, config) = fresh_workspace();
    let throttle = Throttle {
        max_per_window: 5,
        window: ThrottleWindow::PerDay,
    };
    let granted = ThrottleGate::reserve(&config, &"camp_z".into(), Some(&throttle), 0).unwrap();
    assert_eq!(granted, 0);
    let snap = ThrottleGate::current(&config, &"camp_z".into(), Some(&throttle))
        .unwrap()
        .unwrap();
    assert_eq!(snap.consumed, 0);
}
