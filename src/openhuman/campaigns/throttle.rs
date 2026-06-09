//! F4-8 — campaign throttle gate.
//!
//! Persistent budget tracker for `Campaign.throttle`. Buckets
//! reservations by the [`ThrottleWindow`] boundary (midnight UTC for
//! `PerDay`, top-of-hour for `PerHour`, top-of-minute for
//! `PerMinute`) and keeps a SQLite row per `(campaign_id,
//! window_start)` so a core restart picks up the same budget
//! without double-spending.
//!
//! The gate sits in front of every `for_each` iteration that runs
//! under a campaign. The executor calls
//! [`ThrottleGate::reserve`] before each iteration; when the budget
//! is full the executor pauses (in-process sleep for short windows,
//! `pending_resume_at` for `PerDay`). If the iteration fails BEFORE
//! the externally-visible action, the executor calls
//! [`ThrottleGate::release`] so the reservation doesn't burn budget
//! on something that never reached the outside world.

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Timelike, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::openhuman::campaigns::types::{CampaignId, Throttle, ThrottleWindow};
use crate::openhuman::config::Config;
use crate::openhuman::workflows::store::with_connection;

/// In-memory snapshot of a campaign's current throttle state. Used by
/// the `campaigns_throttle_status` RPC so the UI can render
/// "X / Y used today" without forcing the caller through `reserve`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThrottleSnapshot {
    pub window_start: DateTime<Utc>,
    pub window: ThrottleWindow,
    pub consumed: u32,
    pub limit: u32,
    pub remaining: u32,
    /// When the next window opens — useful for "resumes at HH:MM"
    /// copy. Equals `window_start + window_duration`.
    pub next_window_at: DateTime<Utc>,
}

/// Gate that hands out per-campaign throttle reservations. All
/// methods take `&Config` so the gate stays stateless — the only
/// shared state lives in the `campaign_throttle_state` SQLite table.
pub struct ThrottleGate;

impl ThrottleGate {
    /// Try to reserve `n` units of throttle budget for `campaign_id`
    /// against the active window. Returns the actual number reserved
    /// — `0` means the bucket is full and the caller MUST pause or
    /// shed the iteration. Atomic against concurrent callers via
    /// SQLite's `BEGIN IMMEDIATE` transaction.
    ///
    /// `throttle == None` is a no-op — returns `n` without touching
    /// the store. Callers without a throttle don't need the gate.
    pub fn reserve(
        config: &Config,
        campaign_id: &CampaignId,
        throttle: Option<&Throttle>,
        n: u32,
    ) -> Result<u32> {
        let Some(throttle) = throttle else {
            return Ok(n);
        };
        if n == 0 {
            return Ok(0);
        }
        let window_start = current_window_start(throttle.window, Utc::now());
        let max = throttle.max_per_window;
        let cid = campaign_id.clone();
        with_connection(config, |conn| {
            // BEGIN IMMEDIATE serialises concurrent reservers against
            // the same campaign so two sub-workflows can't both
            // observe `available = 1` and both burn the same slot.
            // Raw SQL because `with_connection` hands us `&Connection`
            // and rusqlite's typed transaction needs `&mut`.
            conn.execute_batch("BEGIN IMMEDIATE")
                .context("throttle: begin immediate")?;
            let granted = (|| -> Result<u32> {
                let window_iso = window_start.to_rfc3339();
                conn.execute(
                    "INSERT OR IGNORE INTO campaign_throttle_state \
                     (campaign_id, window_start, consumed) VALUES (?1, ?2, 0)",
                    params![cid, window_iso],
                )?;
                let consumed: u32 = conn
                    .query_row(
                        "SELECT consumed FROM campaign_throttle_state \
                         WHERE campaign_id = ?1 AND window_start = ?2",
                        params![cid, window_iso],
                        |row| row.get::<_, u32>(0),
                    )
                    .context("throttle: read consumed")?;
                let available = max.saturating_sub(consumed);
                let granted = n.min(available);
                if granted > 0 {
                    conn.execute(
                        "UPDATE campaign_throttle_state SET consumed = consumed + ?3 \
                         WHERE campaign_id = ?1 AND window_start = ?2",
                        params![cid, window_iso, granted],
                    )?;
                }
                Ok(granted)
            })();
            match granted {
                Ok(g) => {
                    conn.execute_batch("COMMIT").context("throttle: commit")?;
                    Ok(g)
                }
                Err(e) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(e)
                }
            }
        })
    }

    /// Return `n` units to the budget for the active window.
    /// Floor-clamps at 0 — releasing more than was reserved is a
    /// caller bug but shouldn't crash the run.
    pub fn release(
        config: &Config,
        campaign_id: &CampaignId,
        throttle: Option<&Throttle>,
        n: u32,
    ) -> Result<()> {
        let Some(throttle) = throttle else {
            return Ok(());
        };
        if n == 0 {
            return Ok(());
        }
        let window_start = current_window_start(throttle.window, Utc::now());
        let cid = campaign_id.clone();
        with_connection(config, |conn| {
            // MAX keeps consumed non-negative even if a caller
            // over-releases (release-bigger-than-reserved is a
            // caller bug but shouldn't crash the run).
            conn.execute(
                "UPDATE campaign_throttle_state SET consumed = MAX(0, consumed - ?3) \
                 WHERE campaign_id = ?1 AND window_start = ?2",
                params![cid, window_start.to_rfc3339(), n],
            )?;
            Ok(())
        })
    }

    /// Read-only consumption snapshot. Doesn't mutate; safe to call
    /// from the `campaigns_throttle_status` RPC without holding a
    /// reservation.
    pub fn current(
        config: &Config,
        campaign_id: &CampaignId,
        throttle: Option<&Throttle>,
    ) -> Result<Option<ThrottleSnapshot>> {
        let Some(throttle) = throttle else {
            return Ok(None);
        };
        let now = Utc::now();
        let window_start = current_window_start(throttle.window, now);
        let cid = campaign_id.clone();
        let max = throttle.max_per_window;
        let window = throttle.window;
        let consumed: u32 = with_connection(config, |conn| {
            Ok(conn
                .query_row(
                    "SELECT consumed FROM campaign_throttle_state \
                     WHERE campaign_id = ?1 AND window_start = ?2",
                    params![cid, window_start.to_rfc3339()],
                    |row| row.get::<_, u32>(0),
                )
                .unwrap_or(0))
        })?;
        let next_window_at = window_start + window_duration(window);
        Ok(Some(ThrottleSnapshot {
            window_start,
            window,
            consumed,
            limit: max,
            remaining: max.saturating_sub(consumed),
            next_window_at,
        }))
    }
}

/// Compute the bucket boundary for `now` given a `window`. The
/// `PerDay` bucket starts at the most-recent UTC midnight; `PerHour`
/// at the top of the hour; `PerMinute` at the top of the minute.
pub fn current_window_start(window: ThrottleWindow, now: DateTime<Utc>) -> DateTime<Utc> {
    match window {
        ThrottleWindow::PerDay => Utc
            .with_ymd_and_hms(now.year_ymd().0, now.year_ymd().1, now.year_ymd().2, 0, 0, 0)
            .single()
            .unwrap_or(now),
        ThrottleWindow::PerHour => now
            .with_minute(0)
            .and_then(|t| t.with_second(0))
            .and_then(|t| t.with_nanosecond(0))
            .unwrap_or(now),
        ThrottleWindow::PerMinute => now
            .with_second(0)
            .and_then(|t| t.with_nanosecond(0))
            .unwrap_or(now),
    }
}

/// Compute the duration of a single window. Used to project the
/// `next_window_at` field on [`ThrottleSnapshot`] without re-doing
/// the bucket math.
pub fn window_duration(window: ThrottleWindow) -> chrono::Duration {
    match window {
        ThrottleWindow::PerDay => chrono::Duration::days(1),
        ThrottleWindow::PerHour => chrono::Duration::hours(1),
        ThrottleWindow::PerMinute => chrono::Duration::minutes(1),
    }
}

/// Tiny extension trait so the `Datelike` Y-M-D triple stays
/// readable in `current_window_start`. Avoids three separate method
/// calls inline that all do the same destructuring.
trait YmdExt {
    fn year_ymd(&self) -> (i32, u32, u32);
}

impl YmdExt for DateTime<Utc> {
    fn year_ymd(&self) -> (i32, u32, u32) {
        use chrono::Datelike;
        (self.year(), self.month(), self.day())
    }
}
