//! Persistent schema-change notifications.
//!
//! One JSONL file at `~/.seryai/schema_notifications.jsonl`. Append-only
//! with lazy rotation (keep newest `MAX_NOTIFICATIONS`). Each line is a
//! serialized `StoredNotification` — the schema_changed event payload
//! plus a client-visible id, a received-at timestamp, and a read flag.
//!
//! Writes happen from the scan pipeline in rescan_folder; reads and
//! mutations (mark-read, mark-all-read, clear) come through Tauri
//! commands invoked by the Notifications view. Matches the pattern
//! already used by audit.rs.

use crate::config::Config;
use crate::error::{AgentError, Result};
use crate::schema_diff::SchemaDiff;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use uuid::Uuid;

const MAX_NOTIFICATIONS: usize = 500;

/// How recent an existing notification must be to count as a duplicate.
/// File-watcher bounce from editor saves or brief filesystem churn
/// typically fires within a few seconds; a 60-second window captures
/// that without merging genuinely distinct changes that happen to
/// share a shape.
const DEDUP_WINDOW_SECS: i64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredNotification {
    pub id: String,
    pub received_at: String, // RFC 3339
    pub read: bool,
    pub workspace_id: String,
    pub dataset_path: String,
    pub dataset_name: String,
    pub added: u64,
    pub removed: u64,
    pub type_changed: u64,
    pub diff: SchemaDiff,
}

fn path() -> Result<PathBuf> {
    Ok(Config::data_dir()?.join("schema_notifications.jsonl"))
}

/// Record a fresh schema change.
///
/// Dedup: if a notification for the same (workspace_id, dataset_path)
/// with an identical diff exists and was received within the last
/// DEDUP_WINDOW_SECS, we refresh its received_at timestamp and return
/// the existing record instead of appending a new one. This prevents
/// file-watcher bounce from spamming the tab when the same file
/// churns multiple times in quick succession.
///
/// The dedup check preserves the existing record's `read` flag on
/// purpose — if the user already dismissed the first firing, they
/// don't want it re-appearing as unread three seconds later. If a
/// genuinely new state landing later matters, the next scan will
/// observe a different diff (not dedupable) and surface cleanly.
#[allow(clippy::too_many_arguments)]
pub fn record(
    workspace_id: &str,
    dataset_path: &str,
    dataset_name: &str,
    added: u64,
    removed: u64,
    type_changed: u64,
    diff: SchemaDiff,
) -> Result<StoredNotification> {
    if let Some(existing) = find_recent_duplicate(workspace_id, dataset_path, &diff)? {
        return refresh_received_at(&existing.id).map(|refreshed| refreshed.unwrap_or(existing));
    }

    let entry = StoredNotification {
        id: Uuid::new_v4().to_string(),
        received_at: Utc::now().to_rfc3339(),
        read: false,
        workspace_id: workspace_id.to_string(),
        dataset_path: dataset_path.to_string(),
        dataset_name: dataset_name.to_string(),
        added,
        removed,
        type_changed,
        diff,
    };
    append(&entry)?;
    Ok(entry)
}

/// Look for a stored notification for the same (workspace, path) with
/// an identical diff received in the dedup window. Returns the newest
/// such entry if found. Pure side-effect-free read; safe to call with
/// concurrent mutations because mutate() uses atomic rename.
fn find_recent_duplicate(
    workspace_id: &str,
    dataset_path: &str,
    diff: &SchemaDiff,
) -> Result<Option<StoredNotification>> {
    let p = path()?;
    if !p.exists() {
        return Ok(None);
    }
    let file = fs::File::open(&p).map_err(AgentError::Io)?;
    let reader = BufReader::new(file);
    let cutoff = Utc::now() - chrono::Duration::seconds(DEDUP_WINDOW_SECS);

    // Iterate back-to-front so we see newest entries first.
    let lines: Vec<String> = reader.lines().map_while(|l| l.ok()).collect();
    for line in lines.into_iter().rev() {
        let Ok(entry) = serde_json::from_str::<StoredNotification>(&line) else {
            continue;
        };
        // Timestamps older than the window end the search — entries
        // are written newest-last, so once we cross cutoff going
        // backwards we can stop.
        let Ok(received) = chrono::DateTime::parse_from_rfc3339(&entry.received_at) else {
            continue;
        };
        if received.with_timezone(&Utc) < cutoff {
            return Ok(None);
        }
        if entry.workspace_id == workspace_id
            && entry.dataset_path == dataset_path
            && entry.diff == *diff
        {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

/// Bump the received_at of the entry with the given id. Returns the
/// updated entry if found, None if the id isn't present.
fn refresh_received_at(id: &str) -> Result<Option<StoredNotification>> {
    let p = path()?;
    if !p.exists() {
        return Ok(None);
    }
    let file = fs::File::open(&p).map_err(AgentError::Io)?;
    let reader = BufReader::new(file);
    let mut entries: Vec<StoredNotification> = reader
        .lines()
        .map_while(|l| l.ok())
        .filter_map(|l| serde_json::from_str::<StoredNotification>(&l).ok())
        .collect();

    let mut updated: Option<StoredNotification> = None;
    for e in entries.iter_mut() {
        if e.id == id {
            e.received_at = Utc::now().to_rfc3339();
            updated = Some(e.clone());
            break;
        }
    }
    if updated.is_none() {
        return Ok(None);
    }

    let tmp = p.with_extension("jsonl.tmp");
    {
        let mut out = fs::File::create(&tmp).map_err(AgentError::Io)?;
        for entry in &entries {
            let json = serde_json::to_string(entry)
                .map_err(|e| AgentError::Serialization(format!("schema notification: {}", e)))?;
            writeln!(out, "{}", json).map_err(AgentError::Io)?;
        }
    }
    fs::rename(&tmp, &p).map_err(AgentError::Io)?;
    Ok(updated)
}

fn append(entry: &StoredNotification) -> Result<()> {
    let p = path()?;
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(AgentError::Io)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .map_err(AgentError::Io)?;
    let json = serde_json::to_string(entry)
        .map_err(|e| AgentError::Serialization(format!("schema notification: {}", e)))?;
    writeln!(file, "{}", json).map_err(AgentError::Io)?;
    rotate_if_needed(&p)?;
    Ok(())
}

fn rotate_if_needed(p: &PathBuf) -> Result<()> {
    let file = fs::File::open(p).map_err(AgentError::Io)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().map_while(|l| l.ok()).collect();
    if lines.len() > MAX_NOTIFICATIONS + MAX_NOTIFICATIONS / 10 {
        let keep: Vec<&String> = lines.iter().skip(lines.len() - MAX_NOTIFICATIONS).collect();
        let tmp = p.with_extension("jsonl.tmp");
        {
            let mut out = fs::File::create(&tmp).map_err(AgentError::Io)?;
            for line in keep {
                writeln!(out, "{}", line).map_err(AgentError::Io)?;
            }
        }
        fs::rename(&tmp, p).map_err(AgentError::Io)?;
    }
    Ok(())
}

/// Load notifications newest-first, capped at `limit`.
pub fn load(limit: usize) -> Result<Vec<StoredNotification>> {
    let p = path()?;
    if !p.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&p).map_err(AgentError::Io)?;
    let reader = BufReader::new(file);
    let mut out: Vec<StoredNotification> = Vec::new();
    for line in reader.lines().map_while(|l| l.ok()) {
        if let Ok(entry) = serde_json::from_str::<StoredNotification>(&line) {
            out.push(entry);
        }
        // Skip unparseable lines silently — probably a forward-incompatible
        // version upgrade; better to drop a stale entry than to panic the
        // whole notifications tab.
    }
    out.reverse();
    out.truncate(limit);
    Ok(out)
}

/// Flip `read` to true for a single id. No-op if the id isn't present.
pub fn mark_read(id: &str) -> Result<()> {
    mutate(|entries| {
        for e in entries.iter_mut() {
            if e.id == id {
                e.read = true;
                break;
            }
        }
    })
}

pub fn mark_all_read() -> Result<()> {
    mutate(|entries| {
        for e in entries.iter_mut() {
            e.read = true;
        }
    })
}

pub fn clear() -> Result<()> {
    let p = path()?;
    if p.exists() {
        fs::remove_file(&p).map_err(AgentError::Io)?;
    }
    Ok(())
}

/// Read all, apply the mutation, write back. O(n) but n <= 500 so fine.
/// Write goes to a temp file + rename so a crash can't truncate.
fn mutate<F>(f: F) -> Result<()>
where
    F: FnOnce(&mut Vec<StoredNotification>),
{
    let p = path()?;
    if !p.exists() {
        return Ok(());
    }
    let file = fs::File::open(&p).map_err(AgentError::Io)?;
    let reader = BufReader::new(file);
    let mut entries: Vec<StoredNotification> = reader
        .lines()
        .map_while(|l| l.ok())
        .filter_map(|l| serde_json::from_str::<StoredNotification>(&l).ok())
        .collect();

    f(&mut entries);

    let tmp = p.with_extension("jsonl.tmp");
    {
        let mut out = fs::File::create(&tmp).map_err(AgentError::Io)?;
        for entry in &entries {
            let json = serde_json::to_string(entry)
                .map_err(|e| AgentError::Serialization(format!("schema notification: {}", e)))?;
            writeln!(out, "{}", json).map_err(AgentError::Io)?;
        }
    }
    fs::rename(&tmp, &p).map_err(AgentError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema_diff::{Column, diff_schemas};
    use std::sync::Mutex;

    // All tests in this module share one on-disk JSONL file because
    // Config::data_dir() is a process-global path. Cargo runs tests in
    // parallel by default, so without this mutex tests would race and
    // corrupt each other. Using Mutex<()> over parking_lot to avoid
    // adding a dep for test-only synchronization.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn seed_diff() -> SchemaDiff {
        diff_schemas(
            &[Column {
                name: "amount".into(),
                column_type: "INTEGER".into(),
            }],
            &[
                Column {
                    name: "amount".into(),
                    column_type: "VARCHAR".into(),
                },
                Column {
                    name: "currency".into(),
                    column_type: "VARCHAR".into(),
                },
            ],
        )
    }

    #[test]
    fn record_then_load_roundtrips_and_sorts_newest_first() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Isolate test state in the shared data dir by clearing first.
        clear().unwrap();

        let first = record(
            "ws1",
            "/data/a.parquet",
            "a",
            0,
            0,
            1,
            seed_diff(),
        )
        .unwrap();
        // Ensure the two entries have distinguishable received_at.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let second = record(
            "ws1",
            "/data/b.parquet",
            "b",
            1,
            0,
            0,
            seed_diff(),
        )
        .unwrap();

        let loaded = load(100).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, second.id, "newest entry comes first");
        assert_eq!(loaded[1].id, first.id);
        assert!(loaded.iter().all(|e| !e.read));

        clear().unwrap();
    }

    #[test]
    fn mark_read_flips_one_entry_and_survives_load() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear().unwrap();
        let a = record("ws", "/a", "a", 1, 0, 0, seed_diff()).unwrap();
        let b = record("ws", "/b", "b", 1, 0, 0, seed_diff()).unwrap();

        mark_read(&a.id).unwrap();
        let loaded = load(100).unwrap();

        let a_loaded = loaded.iter().find(|e| e.id == a.id).unwrap();
        let b_loaded = loaded.iter().find(|e| e.id == b.id).unwrap();
        assert!(a_loaded.read, "a should be read");
        assert!(!b_loaded.read, "b should still be unread");

        clear().unwrap();
    }

    #[test]
    fn mark_all_read_flips_every_entry() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear().unwrap();
        record("ws", "/a", "a", 1, 0, 0, seed_diff()).unwrap();
        record("ws", "/b", "b", 1, 0, 0, seed_diff()).unwrap();
        record("ws", "/c", "c", 1, 0, 0, seed_diff()).unwrap();

        mark_all_read().unwrap();
        let loaded = load(100).unwrap();
        assert_eq!(loaded.len(), 3);
        assert!(loaded.iter().all(|e| e.read));

        clear().unwrap();
    }

    #[test]
    fn mark_read_unknown_id_is_noop() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear().unwrap();
        record("ws", "/a", "a", 1, 0, 0, seed_diff()).unwrap();
        mark_read("not-a-real-id").unwrap();
        let loaded = load(100).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(!loaded[0].read);

        clear().unwrap();
    }

    #[test]
    fn clear_removes_all_and_load_returns_empty() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear().unwrap();
        record("ws", "/a", "a", 1, 0, 0, seed_diff()).unwrap();
        clear().unwrap();
        assert!(load(100).unwrap().is_empty());
    }

    #[test]
    fn load_when_file_missing_returns_empty() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear().unwrap();
        assert!(load(100).unwrap().is_empty());
    }

    #[test]
    fn dedup_same_diff_within_window_does_not_add_new_entry() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear().unwrap();

        let first = record(
            "ws",
            "/data/orders.parquet",
            "orders.parquet",
            1,
            0,
            1,
            seed_diff(),
        )
        .unwrap();
        // Immediate re-record with identical diff — simulates file-watcher
        // bounce firing twice for the same save.
        let second = record(
            "ws",
            "/data/orders.parquet",
            "orders.parquet",
            1,
            0,
            1,
            seed_diff(),
        )
        .unwrap();

        assert_eq!(
            second.id, first.id,
            "second record should reuse the first's id"
        );
        let loaded = load(100).unwrap();
        assert_eq!(loaded.len(), 1, "no duplicate entry should be written");

        clear().unwrap();
    }

    #[test]
    fn dedup_preserves_read_flag_on_refresh() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear().unwrap();

        let first = record("ws", "/a", "a", 1, 0, 0, seed_diff()).unwrap();
        mark_read(&first.id).unwrap();

        // Bounce: same diff again. The stored record should stay read —
        // the user already dismissed it, re-surfacing would be annoying.
        record("ws", "/a", "a", 1, 0, 0, seed_diff()).unwrap();
        let loaded = load(100).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(
            loaded[0].read,
            "read flag must be preserved across dedup refresh"
        );

        clear().unwrap();
    }

    #[test]
    fn different_diff_for_same_path_is_a_new_entry() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear().unwrap();

        let first_diff = diff_schemas(
            &[Column {
                name: "amount".into(),
                column_type: "INTEGER".into(),
            }],
            &[Column {
                name: "amount".into(),
                column_type: "VARCHAR".into(),
            }],
        );
        let second_diff = diff_schemas(
            &[Column {
                name: "amount".into(),
                column_type: "VARCHAR".into(),
            }],
            &[
                Column {
                    name: "amount".into(),
                    column_type: "VARCHAR".into(),
                },
                Column {
                    name: "currency".into(),
                    column_type: "VARCHAR".into(),
                },
            ],
        );

        record("ws", "/o", "o", 0, 0, 1, first_diff).unwrap();
        record("ws", "/o", "o", 1, 0, 0, second_diff).unwrap();

        let loaded = load(100).unwrap();
        assert_eq!(
            loaded.len(),
            2,
            "different diffs on the same path must produce distinct entries"
        );

        clear().unwrap();
    }

    #[test]
    fn dedup_differentiates_by_workspace_id() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear().unwrap();

        // Same diff, same path, but different workspace — must be
        // treated as distinct notifications.
        record("ws-1", "/a", "a", 1, 0, 0, seed_diff()).unwrap();
        record("ws-2", "/a", "a", 1, 0, 0, seed_diff()).unwrap();

        let loaded = load(100).unwrap();
        assert_eq!(loaded.len(), 2);

        clear().unwrap();
    }
}
