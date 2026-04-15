// Plugin Marketplace
//
// Community plugin discovery and installation.
// - Local registry format (marketplace.json)
// - Plugin search/filter
// - Installation from URL or local path
// - Version compatibility checking

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AgentError, Result};
use crate::plugin::PluginManifest;

/// Marketplace entry for a discoverable plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceEntry {
    /// Plugin metadata from manifest
    pub manifest: PluginManifest,

    /// Download/install source
    pub source: PluginSource,

    /// Community metrics
    pub metrics: PluginMetrics,

    /// Additional metadata
    pub featured: bool,
    pub verified: bool, // Verified by Sery team
    pub tags: Vec<String>,
    pub screenshots: Vec<String>,
    pub changelog_url: Option<String>,
    pub repo_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PluginSource {
    /// GitHub release URL
    GitHub {
        owner: String,
        repo: String,
        tag: String, // Release tag or "latest"
    },
    /// Direct download URL
    Url { url: String },
    /// Local filesystem path (for development)
    Local { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginMetrics {
    pub downloads: u64,
    pub stars: u64,
    pub rating: f32, // 0.0 - 5.0
    pub review_count: u32,
    pub last_updated: String, // ISO 8601
}

/// Plugin marketplace registry
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarketplaceRegistry {
    pub version: String,
    pub last_updated: String,
    pub plugins: Vec<MarketplaceEntry>,
}

impl MarketplaceRegistry {
    /// Load marketplace from JSON file
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .map_err(|e| AgentError::FileSystem(format!("Failed to read marketplace: {}", e)))?;

        serde_json::from_str(&content)
            .map_err(|e| AgentError::Serialization(format!("Invalid marketplace format: {}", e)))
    }

    /// Save marketplace to JSON file
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| AgentError::Serialization(format!("Failed to serialize marketplace: {}", e)))?;

        fs::write(path, json)
            .map_err(|e| AgentError::FileSystem(format!("Failed to write marketplace: {}", e)))?;

        Ok(())
    }

    /// Search plugins by name, description, tags, or capabilities
    pub fn search(&self, query: &str) -> Vec<&MarketplaceEntry> {
        let query_lower = query.to_lowercase();

        self.plugins
            .iter()
            .filter(|entry| {
                entry.manifest.name.to_lowercase().contains(&query_lower)
                    || entry
                        .manifest
                        .description
                        .to_lowercase()
                        .contains(&query_lower)
                    || entry
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&query_lower))
                    || entry
                        .manifest
                        .capabilities
                        .iter()
                        .any(|cap| format!("{:?}", cap).to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    /// Filter by capability
    pub fn by_capability(&self, capability: &str) -> Vec<&MarketplaceEntry> {
        self.plugins
            .iter()
            .filter(|entry| {
                entry
                    .manifest
                    .capabilities
                    .iter()
                    .any(|cap| format!("{:?}", cap).to_lowercase() == capability.to_lowercase())
            })
            .collect()
    }

    /// Filter by tag
    pub fn by_tag(&self, tag: &str) -> Vec<&MarketplaceEntry> {
        let tag_lower = tag.to_lowercase();
        self.plugins
            .iter()
            .filter(|entry| {
                entry
                    .tags
                    .iter()
                    .any(|t| t.to_lowercase() == tag_lower)
            })
            .collect()
    }

    /// Get featured plugins
    pub fn featured(&self) -> Vec<&MarketplaceEntry> {
        self.plugins
            .iter()
            .filter(|entry| entry.featured)
            .collect()
    }

    /// Get verified plugins
    pub fn verified(&self) -> Vec<&MarketplaceEntry> {
        self.plugins
            .iter()
            .filter(|entry| entry.verified)
            .collect()
    }

    /// Sort by popularity (downloads)
    pub fn popular(&self, limit: usize) -> Vec<&MarketplaceEntry> {
        let mut plugins: Vec<&MarketplaceEntry> = self.plugins.iter().collect();
        plugins.sort_by(|a, b| b.metrics.downloads.cmp(&a.metrics.downloads));
        plugins.into_iter().take(limit).collect()
    }

    /// Sort by rating
    pub fn top_rated(&self, limit: usize) -> Vec<&MarketplaceEntry> {
        let mut plugins: Vec<&MarketplaceEntry> = self.plugins.iter().collect();
        plugins.sort_by(|a, b| {
            b.metrics
                .rating
                .partial_cmp(&a.metrics.rating)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        plugins.into_iter().take(limit).collect()
    }

    /// Get plugin by ID
    pub fn get(&self, plugin_id: &str) -> Option<&MarketplaceEntry> {
        self.plugins
            .iter()
            .find(|entry| entry.manifest.id == plugin_id)
    }

    /// Add or update plugin in marketplace
    pub fn upsert(&mut self, entry: MarketplaceEntry) {
        if let Some(existing) = self
            .plugins
            .iter_mut()
            .find(|e| e.manifest.id == entry.manifest.id)
        {
            *existing = entry;
        } else {
            self.plugins.push(entry);
        }
    }

    /// Remove plugin from marketplace
    pub fn remove(&mut self, plugin_id: &str) -> bool {
        let initial_len = self.plugins.len();
        self.plugins.retain(|e| e.manifest.id != plugin_id);
        self.plugins.len() < initial_len
    }
}

/// Plugin installer
pub struct PluginInstaller {
    plugins_dir: PathBuf,
}

impl PluginInstaller {
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self { plugins_dir }
    }

    /// Install plugin from marketplace entry
    pub async fn install(&self, entry: &MarketplaceEntry) -> Result<()> {
        let plugin_dir = self.plugins_dir.join(&entry.manifest.id);

        // Create plugin directory
        fs::create_dir_all(&plugin_dir)
            .map_err(|e| AgentError::FileSystem(format!("Failed to create plugin dir: {}", e)))?;

        match &entry.source {
            PluginSource::Local { path } => {
                self.install_from_local(path, &plugin_dir)?;
            }
            PluginSource::Url { url } => {
                self.install_from_url(url, &plugin_dir).await?;
            }
            PluginSource::GitHub { owner, repo, tag } => {
                self.install_from_github(owner, repo, tag, &plugin_dir)
                    .await?;
            }
        }

        // Write manifest
        let manifest_path = plugin_dir.join("plugin.json");
        let manifest_json = serde_json::to_string_pretty(&entry.manifest)
            .map_err(|e| AgentError::Serialization(format!("Failed to serialize manifest: {}", e)))?;
        fs::write(manifest_path, manifest_json)
            .map_err(|e| AgentError::FileSystem(format!("Failed to write manifest: {}", e)))?;

        Ok(())
    }

    fn install_from_local(&self, source_path: &str, dest_dir: &Path) -> Result<()> {
        let source = PathBuf::from(source_path);

        // Copy plugin.wasm
        let wasm_src = source.join("plugin.wasm");
        let wasm_dest = dest_dir.join("plugin.wasm");
        fs::copy(&wasm_src, &wasm_dest).map_err(|e| {
            AgentError::FileSystem(format!("Failed to copy plugin.wasm: {}", e))
        })?;

        // Copy README if exists
        let readme_src = source.join("README.md");
        if readme_src.exists() {
            let readme_dest = dest_dir.join("README.md");
            fs::copy(&readme_src, &readme_dest).ok();
        }

        Ok(())
    }

    async fn install_from_url(&self, url: &str, dest_dir: &Path) -> Result<()> {
        // TODO: Implement HTTP download
        // For now, return error
        Err(AgentError::Validation(
            "URL installation not yet implemented".to_string(),
        ))
    }

    async fn install_from_github(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
        dest_dir: &Path,
    ) -> Result<()> {
        // TODO: Implement GitHub release download
        // For now, return error
        Err(AgentError::Validation(
            "GitHub installation not yet implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginCapability;
    use tempfile::TempDir;

    fn test_entry(id: &str, name: &str, downloads: u64) -> MarketplaceEntry {
        MarketplaceEntry {
            manifest: PluginManifest {
                id: id.to_string(),
                name: name.to_string(),
                version: "1.0.0".to_string(),
                author: "Test Author".to_string(),
                description: format!("{} plugin", name),
                capabilities: vec![PluginCapability::DataSource],
                permissions: vec![],
                entry_point: "plugin.wasm".to_string(),
                icon: None,
                homepage: None,
                functions: None,
            },
            source: PluginSource::Local {
                path: format!("/plugins/{}", id),
            },
            metrics: PluginMetrics {
                downloads,
                stars: 100,
                rating: 4.5,
                review_count: 10,
                last_updated: "2024-01-01T00:00:00Z".to_string(),
            },
            featured: false,
            verified: false,
            tags: vec!["test".to_string()],
            screenshots: vec![],
            changelog_url: None,
            repo_url: None,
        }
    }

    #[test]
    fn test_marketplace_search() {
        let mut registry = MarketplaceRegistry::default();
        registry.plugins.push(test_entry("com.test.csv", "CSV Parser", 1000));
        registry.plugins.push(test_entry("com.test.json", "JSON Transformer", 500));
        registry.plugins.push(test_entry("com.test.xml", "XML Viewer", 250));

        let results = registry.search("csv");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].manifest.name, "CSV Parser");

        let results = registry.search("json");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].manifest.name, "JSON Transformer");
    }

    #[test]
    fn test_marketplace_popular() {
        let mut registry = MarketplaceRegistry::default();
        registry.plugins.push(test_entry("com.test.a", "Plugin A", 1000));
        registry.plugins.push(test_entry("com.test.b", "Plugin B", 5000));
        registry.plugins.push(test_entry("com.test.c", "Plugin C", 2500));

        let popular = registry.popular(2);
        assert_eq!(popular.len(), 2);
        assert_eq!(popular[0].manifest.name, "Plugin B"); // 5000 downloads
        assert_eq!(popular[1].manifest.name, "Plugin C"); // 2500 downloads
    }

    #[test]
    fn test_marketplace_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().join("marketplace.json");

        let mut registry = MarketplaceRegistry::default();
        registry.version = "1.0".to_string();
        registry.plugins.push(test_entry("com.test.plugin", "Test Plugin", 100));

        // Save
        registry.save(&registry_path).unwrap();
        assert!(registry_path.exists());

        // Load
        let loaded = MarketplaceRegistry::load(&registry_path).unwrap();
        assert_eq!(loaded.version, "1.0");
        assert_eq!(loaded.plugins.len(), 1);
        assert_eq!(loaded.plugins[0].manifest.id, "com.test.plugin");
    }
}
