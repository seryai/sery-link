//! Local agent statistics — persisted to `~/.seryai/stats.json`.
//!
//! Keeps a running tally of how many queries the agent has served so the
//! status bar and dashboard can show "X queries today" / "Y queries total"
//! without polling the cloud. Refreshed whenever a query completes.

use crate::config::Config;
use crate::error::{AgentError, Result};
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

static STATS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Stats {
    pub total_queries: u64,
    pub queries_today: u64,
    pub queries_today_date: Option<String>, // YYYY-MM-DD
    pub successful_queries: u64,
    pub failed_queries: u64,
    pub total_bytes_read: u64,
    pub last_query_at: Option<String>,
    pub uptime_started_at: Option<String>,
}

fn path() -> Result<PathBuf> {
    Ok(Config::data_dir()?.join("stats.json"))
}

pub fn load() -> Result<Stats> {
    let p = path()?;
    if !p.exists() {
        return Ok(Stats::default());
    }
    let contents = fs::read_to_string(&p).map_err(AgentError::Io)?;
    // Tolerate stale/corrupt files by falling back to defaults rather than
    // erroring — stats are non-critical and should never block the agent.
    Ok(serde_json::from_str(&contents).unwrap_or_default())
}

fn save(stats: &Stats) -> Result<()> {
    let p = path()?;
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(AgentError::Io)?;
    }
    let contents = serde_json::to_string_pretty(stats)
        .map_err(|e| AgentError::Config(format!("serialize stats: {}", e)))?;
    fs::write(&p, contents).map_err(AgentError::Io)?;
    Ok(())
}

/// Atomically update stats via a closure. Uses a process-local mutex to
/// serialize concurrent updaters from multiple async tasks. Not cross-
/// process safe, but the Sery Link agent is single-process.
pub fn update<F>(f: F) -> Result<Stats>
where
    F: FnOnce(&mut Stats),
{
    let _guard = STATS_LOCK.lock().unwrap();
    let mut stats = load()?;
    f(&mut stats);
    save(&stats)?;
    Ok(stats)
}

/// Record a successful query — rolls over the daily counter if the day has
/// changed since the last query was recorded.
pub fn record_query_success(duration_ms: u64, row_count: Option<usize>) -> Result<()> {
    let now = Utc::now();
    let today = today_key(&now);
    update(|s| {
        roll_over_day(s, &today);
        s.total_queries += 1;
        s.queries_today += 1;
        s.successful_queries += 1;
        s.last_query_at = Some(now.to_rfc3339());
        // Approximate byte budget — DuckDB doesn't report scan bytes directly,
        // so we use a proxy: ~100 bytes per row (tunable). Only used as a
        // rough "data touched" indicator in the dashboard.
        if let Some(rc) = row_count {
            s.total_bytes_read = s.total_bytes_read.saturating_add((rc as u64) * 100);
        }
        let _ = duration_ms; // currently unused, reserved for avg duration tracking
    })?;
    Ok(())
}

pub fn record_query_failure() -> Result<()> {
    let now = Utc::now();
    let today = today_key(&now);
    update(|s| {
        roll_over_day(s, &today);
        s.total_queries += 1;
        s.queries_today += 1;
        s.failed_queries += 1;
        s.last_query_at = Some(now.to_rfc3339());
    })?;
    Ok(())
}

fn roll_over_day(stats: &mut Stats, today: &str) {
    let rollover = match &stats.queries_today_date {
        Some(d) => d != today,
        None => true,
    };
    if rollover {
        stats.queries_today = 0;
        stats.queries_today_date = Some(today.to_string());
    }
}

fn today_key(now: &DateTime<Utc>) -> String {
    format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day())
}

pub fn record_startup() -> Result<()> {
    update(|s| {
        s.uptime_started_at = Some(Utc::now().to_rfc3339());
    })?;
    Ok(())
}

#[allow(dead_code)]
pub fn clear() -> Result<()> {
    let _guard = STATS_LOCK.lock().unwrap();
    let p = path()?;
    if p.exists() {
        fs::remove_file(&p).map_err(AgentError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{roll_over_day, today_key, Stats};
    use chrono::TimeZone;

    #[test]
    fn today_key_formats_as_iso_date() {
        let dt = chrono::Utc
            .with_ymd_and_hms(2026, 7, 13, 9, 0, 0)
            .unwrap();
        assert_eq!(today_key(&dt), "2026-07-13");
    }

    #[test]
    fn today_key_zero_pads_single_digit_month_and_day() {
        let dt = chrono::Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap();
        assert_eq!(today_key(&dt), "2026-01-05");
    }

    #[test]
    fn roll_over_initializes_date_on_first_call() {
        let mut s = Stats::default();
        s.queries_today = 42; // pretend state from a crash or bug
        roll_over_day(&mut s, "2026-04-17");
        assert_eq!(s.queries_today, 0, "counter resets on first roll-over");
        assert_eq!(s.queries_today_date.as_deref(), Some("2026-04-17"));
    }

    #[test]
    fn roll_over_resets_counter_on_new_day() {
        let mut s = Stats::default();
        s.queries_today = 7;
        s.queries_today_date = Some("2026-04-16".into());
        roll_over_day(&mut s, "2026-04-17");
        assert_eq!(s.queries_today, 0);
        assert_eq!(s.queries_today_date.as_deref(), Some("2026-04-17"));
    }

    #[test]
    fn roll_over_is_noop_on_same_day() {
        let mut s = Stats::default();
        s.queries_today = 7;
        s.queries_today_date = Some("2026-04-17".into());
        roll_over_day(&mut s, "2026-04-17");
        assert_eq!(s.queries_today, 7, "counter must not reset mid-day");
        assert_eq!(s.queries_today_date.as_deref(), Some("2026-04-17"));
    }
}
