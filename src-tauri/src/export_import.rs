//! Configuration export and import — enables backup/restore of agent state.
//!
//! Export format (JSON):
//! {
//!   "version": "1.0",
//!   "exported_at": "2024-01-15T10:30:00Z",
//!   "workspace_id": "workspace-uuid",
//!   "config": { ... agent config ... },
//!   "datasets": [ ... cached datasets ... ],
//!   "query_history": [ ... query history entries ... ]
//! }
//!
//! Import strategies:
//! - MERGE: Keep existing folders, add new ones (default)
//! - OVERWRITE: Replace all configuration
//! - SKIP_DUPLICATES: Only add folders that don't exist

use crate::config::{Config, WatchedFolder};
use crate::error::{AgentError, Result};
use crate::metadata_cache::{CachedDataset, MetadataCache};
use crate::history::{self, QueryHistoryEntry};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Export format version (semver)
const EXPORT_VERSION: &str = "1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportData {
    /// Format version for compatibility checking
    pub version: String,
    /// Timestamp when this export was created
    pub exported_at: DateTime<Utc>,
    /// Workspace ID (for validation on import)
    pub workspace_id: String,
    /// Watched folders
    pub watched_folders: Vec<WatchedFolder>,
    /// Cached dataset metadata
    pub datasets: Vec<CachedDataset>,
    /// Query history (last 500 entries)
    pub query_history: Vec<QueryHistoryEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportStrategy {
    /// Merge imported folders with existing ones (keep both)
    Merge,
    /// Replace entire configuration with imported data
    Overwrite,
    /// Only add folders that don't already exist
    SkipDuplicates,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    /// Number of folders added
    pub folders_added: usize,
    /// Number of folders skipped (duplicates)
    pub folders_skipped: usize,
    /// Number of folders replaced (overwrite mode)
    pub folders_replaced: usize,
    /// Number of datasets imported to cache
    pub datasets_imported: usize,
    /// Number of query history entries imported
    pub queries_imported: usize,
    /// Warnings (e.g., version mismatch, missing data)
    pub warnings: Vec<String>,
}

/// Export agent configuration and metadata to JSON
pub fn export_config(workspace_id: &str, config: &Config) -> Result<ExportData> {
    // Load cached datasets
    let cache = MetadataCache::new()?;
    let datasets = cache.get_all(workspace_id)?;

    // Load query history (last 500 entries)
    let query_history = history::load_history(500).unwrap_or_default();

    Ok(ExportData {
        version: EXPORT_VERSION.to_string(),
        exported_at: Utc::now(),
        workspace_id: workspace_id.to_string(),
        watched_folders: config.watched_folders.clone(),
        datasets,
        query_history,
    })
}

/// Export to JSON string
pub fn export_to_json(workspace_id: &str, config: &Config) -> Result<String> {
    let export_data = export_config(workspace_id, config)?;
    serde_json::to_string_pretty(&export_data)
        .map_err(|e| AgentError::Serialization(format!("Failed to serialize export: {}", e)))
}

/// Import configuration from JSON string
pub fn import_from_json(
    json: &str,
    current_workspace_id: &str,
    current_folders: &[WatchedFolder],
    strategy: ImportStrategy,
) -> Result<(Vec<WatchedFolder>, ImportResult)> {
    // Parse JSON
    let import_data: ExportData = serde_json::from_str(json)
        .map_err(|e| AgentError::Serialization(format!("Invalid export format: {}", e)))?;

    // Validate version compatibility
    let mut warnings = Vec::new();
    if import_data.version != EXPORT_VERSION {
        warnings.push(format!(
            "Version mismatch: export is v{}, current is v{}. Import may be incomplete.",
            import_data.version, EXPORT_VERSION
        ));
    }

    // Warn if workspace IDs don't match
    if import_data.workspace_id != current_workspace_id {
        warnings.push(format!(
            "Workspace ID mismatch: importing from '{}' into '{}'",
            import_data.workspace_id, current_workspace_id
        ));
    }

    // Apply import strategy
    let (new_folders, folders_added, folders_skipped, folders_replaced) = match strategy {
        ImportStrategy::Overwrite => {
            // Replace entire folder list
            let count = import_data.watched_folders.len();
            (import_data.watched_folders.clone(), count, 0, count)
        }
        ImportStrategy::Merge => {
            // Merge folders (keep existing + add new)
            let mut merged_folders = current_folders.to_vec();
            let mut added = 0;
            let mut skipped = 0;

            let existing_paths: HashSet<_> = current_folders
                .iter()
                .map(|f| f.path.clone())
                .collect();

            for folder in import_data.watched_folders {
                if existing_paths.contains(&folder.path) {
                    skipped += 1;
                } else {
                    merged_folders.push(folder);
                    added += 1;
                }
            }

            (merged_folders, added, skipped, 0)
        }
        ImportStrategy::SkipDuplicates => {
            // Only add non-existing folders
            let mut merged_folders = current_folders.to_vec();
            let mut added = 0;
            let mut skipped = 0;

            let existing_paths: HashSet<_> = current_folders
                .iter()
                .map(|f| f.path.clone())
                .collect();

            for folder in import_data.watched_folders {
                if existing_paths.contains(&folder.path) {
                    skipped += 1;
                } else {
                    merged_folders.push(folder);
                    added += 1;
                }
            }

            (merged_folders, added, skipped, 0)
        }
    };

    // Import datasets to metadata cache
    let datasets_imported = if !import_data.datasets.is_empty() {
        let mut cache = MetadataCache::new()?;
        cache.upsert_many(&import_data.datasets)?;
        import_data.datasets.len()
    } else {
        0
    };

    // Import query history
    let queries_imported = if !import_data.query_history.is_empty() {
        // TODO: Implement query history import when we have a local query history store
        // For now, just count them
        warnings.push("Query history import not yet implemented".to_string());
        0
    } else {
        0
    };

    let result = ImportResult {
        folders_added,
        folders_skipped,
        folders_replaced,
        datasets_imported,
        queries_imported,
        warnings,
    };

    Ok((new_folders, result))
}

/// Validate export JSON without importing
pub fn validate_export(json: &str) -> Result<ExportData> {
    serde_json::from_str(json)
        .map_err(|e| AgentError::Serialization(format!("Invalid export format: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_folder() -> WatchedFolder {
        WatchedFolder {
            path: "/test/data".to_string(),
            recursive: true,
            exclude_patterns: vec![],
            max_file_size_mb: 1024,
            last_scan_at: None,
            last_scan_stats: None,
            mcp_enabled: false,
        }
    }

    fn test_config() -> Config {
        Config {
            agent: crate::config::AgentConfig {
                name: "test-agent".to_string(),
                platform: "test".to_string(),
                hostname: "test-host".to_string(),
                agent_id: None,
                workspace_id: None,
            },
            watched_folders: vec![test_folder()],
            cloud: crate::config::CloudConfig {
                api_url: "http://localhost:8000".to_string(),
                websocket_url: "ws://localhost:8000/ws".to_string(),
                web_url: "http://localhost:3000".to_string(),
            },
            sync: crate::config::SyncConfig {
                interval_seconds: 300,
                auto_sync_on_change: true,
                fallback_scan_interval_seconds: 3600,
                scan_tier_overrides: std::collections::HashMap::new(),
                include_document_text: false,
            },
            app: crate::config::AppConfig {
                theme: "system".to_string(),
                launch_at_login: false,
                auto_update: true,
                notifications_enabled: true,
                first_run_completed: false,
                window_hide_notified: false,
                selected_auth_mode: None,
                schema_change_toasts_enabled: true,
            },
        }
    }

    #[test]
    fn test_export_import_roundtrip() {
        let workspace_id = "test-workspace";
        let config = test_config();

        // Export
        let json = export_to_json(workspace_id, &config).unwrap();
        assert!(json.contains("\"version\":"));
        assert!(json.contains("test-workspace"));

        // Import with merge
        let (imported_folders, result) =
            import_from_json(&json, workspace_id, &[], ImportStrategy::Merge)
                .unwrap();

        assert_eq!(imported_folders.len(), 1);
        assert_eq!(imported_folders[0].path, "/test/data");
        assert_eq!(result.folders_added, 1);
        assert_eq!(result.folders_skipped, 0);
    }

    #[test]
    fn test_import_merge_strategy() {
        let workspace_id = "test-workspace";
        let current_folders = vec![
            test_folder(),
            WatchedFolder {
                path: "/existing/folder".to_string(),
                recursive: true,
                exclude_patterns: vec![],
                max_file_size_mb: 1024,
                last_scan_at: None,
                last_scan_stats: None,
                mcp_enabled: false,
            },
        ];

        let import_config = test_config();
        let json = export_to_json(workspace_id, &import_config).unwrap();

        // Import with merge (should skip duplicate /test/data)
        let (merged_folders, result) =
            import_from_json(&json, workspace_id, &current_folders, ImportStrategy::Merge).unwrap();

        assert_eq!(merged_folders.len(), 2); // existing + no duplicate
        assert_eq!(result.folders_added, 0); // /test/data already exists
        assert_eq!(result.folders_skipped, 1);
    }

    #[test]
    fn test_import_overwrite_strategy() {
        let workspace_id = "test-workspace";
        let current_folders = vec![WatchedFolder {
            path: "/old/folder".to_string(),
            recursive: true,
            exclude_patterns: vec![],
            max_file_size_mb: 1024,
            last_scan_at: None,
            last_scan_stats: None,
            mcp_enabled: false,
        }];

        let import_config = test_config();
        let json = export_to_json(workspace_id, &import_config).unwrap();

        // Import with overwrite
        let (new_folders, result) = import_from_json(
            &json,
            workspace_id,
            &current_folders,
            ImportStrategy::Overwrite,
        )
        .unwrap();

        assert_eq!(new_folders.len(), 1);
        assert_eq!(new_folders[0].path, "/test/data"); // replaced
        assert_eq!(result.folders_replaced, 1);
    }

    #[test]
    fn test_version_warning() {
        let export_data = ExportData {
            version: "0.9".to_string(), // Old version
            exported_at: Utc::now(),
            workspace_id: "test".to_string(),
            watched_folders: vec![test_folder()],
            datasets: vec![],
            query_history: vec![],
        };

        let json = serde_json::to_string(&export_data).unwrap();
        let (_, result) =
            import_from_json(&json, "test", &[], ImportStrategy::Merge)
                .unwrap();

        assert!(result.warnings.iter().any(|w| w.contains("Version mismatch")));
    }
}
