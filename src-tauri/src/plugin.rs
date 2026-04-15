//! Plugin system for Sery Link
//!
//! Enables community extensions to add capabilities:
//! - Custom data sources (new file formats, APIs, databases)
//! - Data transformations (aggregations, joins, filters)
//! - Visualizations (charts, graphs, custom renderers)
//! - Export formats (PDF, Excel, custom formats)
//!
//! Plugin manifest format (plugin.json):
//! {
//!   "id": "com.example.csv-viewer",
//!   "name": "Enhanced CSV Viewer",
//!   "version": "1.0.0",
//!   "author": "Jane Doe",
//!   "description": "Advanced CSV file viewer with syntax highlighting",
//!   "capabilities": ["data-source", "viewer"],
//!   "permissions": ["read-files"],
//!   "entry_point": "plugin.wasm",
//!   "icon": "icon.png"
//! }
//!
//! Plugins are loaded from ~/.sery/plugins/[plugin-id]/

use crate::error::{AgentError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Plugin function parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginFunctionParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
}

/// Plugin function metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginFunction {
    pub name: String,
    pub description: String,
    pub parameters: Vec<PluginFunctionParameter>,
    pub returns: String,
    pub requires_file: bool,
}

/// Plugin manifest schema (plugin.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Unique identifier (reverse-DNS format: com.example.plugin-name)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Semantic version (e.g., "1.0.0")
    pub version: String,
    /// Plugin author
    pub author: String,
    /// Short description (max 200 chars)
    pub description: String,
    /// Capabilities this plugin provides
    pub capabilities: Vec<PluginCapability>,
    /// Permissions required by this plugin
    pub permissions: Vec<PluginPermission>,
    /// Entry point file (WebAssembly module)
    pub entry_point: String,
    /// Optional icon file (PNG, 128x128)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Optional website/repository URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Available functions exposed by this plugin
    #[serde(skip_serializing_if = "Option::is_none")]
    pub functions: Option<Vec<PluginFunction>>,
}

/// Plugin capabilities (what the plugin can do)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginCapability {
    /// Adds support for a new file format or data source
    DataSource,
    /// Provides a custom viewer/renderer for data
    Viewer,
    /// Performs data transformations
    Transform,
    /// Exports data to a custom format
    Exporter,
    /// Adds UI components
    UiComponent,
}

/// Plugin permissions (what the plugin needs access to)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginPermission {
    /// Read files from watched folders
    ReadFiles,
    /// Execute external commands
    ExecuteCommands,
    /// Make network requests
    Network,
    /// Access clipboard
    Clipboard,
}

/// Plugin registry entry (stores plugin state)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRegistryEntry {
    pub id: String,
    pub enabled: bool,
    pub installed_at: chrono::DateTime<chrono::Utc>,
    pub version: String,
}

/// Plugin manager
pub struct PluginManager {
    plugins_dir: PathBuf,
    registry_path: PathBuf,
    registry: HashMap<String, PluginRegistryEntry>,
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new() -> Result<Self> {
        let plugins_dir = Self::get_plugins_dir()?;
        let registry_path = plugins_dir.join("registry.json");

        // Ensure plugins directory exists
        if !plugins_dir.exists() {
            fs::create_dir_all(&plugins_dir)
                .map_err(|e| AgentError::FileSystem(format!("Failed to create plugins dir: {}", e)))?;
        }

        // Load registry
        let registry = if registry_path.exists() {
            let contents = fs::read_to_string(&registry_path)
                .map_err(|e| AgentError::FileSystem(format!("Failed to read registry: {}", e)))?;
            serde_json::from_str(&contents)
                .map_err(|e| AgentError::Serialization(format!("Invalid registry: {}", e)))?
        } else {
            HashMap::new()
        };

        Ok(Self {
            plugins_dir,
            registry_path,
            registry,
        })
    }

    /// Get the plugins directory path
    fn get_plugins_dir() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| AgentError::Config("Could not find home directory".to_string()))?;
        Ok(home.join(".sery").join("plugins"))
    }

    /// Discover all plugins in the plugins directory
    pub fn discover_plugins(&self) -> Result<Vec<PluginManifest>> {
        let mut manifests = Vec::new();

        if !self.plugins_dir.exists() {
            return Ok(manifests);
        }

        for entry in fs::read_dir(&self.plugins_dir)
            .map_err(|e| AgentError::FileSystem(format!("Failed to read plugins dir: {}", e)))?
        {
            let entry = entry.map_err(|e| AgentError::FileSystem(format!("Failed to read entry: {}", e)))?;
            let path = entry.path();

            // Skip files, only look at directories
            if !path.is_dir() {
                continue;
            }

            // Skip registry.json
            if path.file_name() == Some(std::ffi::OsStr::new("registry.json")) {
                continue;
            }

            // Try to load plugin.json
            let manifest_path = path.join("plugin.json");
            if manifest_path.exists() {
                match self.load_manifest(&manifest_path) {
                    Ok(manifest) => manifests.push(manifest),
                    Err(e) => {
                        eprintln!("Failed to load plugin manifest at {:?}: {}", manifest_path, e);
                    }
                }
            }
        }

        Ok(manifests)
    }

    /// Load a plugin manifest from a file
    fn load_manifest(&self, path: &Path) -> Result<PluginManifest> {
        let contents = fs::read_to_string(path)
            .map_err(|e| AgentError::FileSystem(format!("Failed to read manifest: {}", e)))?;

        let manifest: PluginManifest = serde_json::from_str(&contents)
            .map_err(|e| AgentError::Serialization(format!("Invalid manifest: {}", e)))?;

        // Validate manifest
        self.validate_manifest(&manifest)?;

        Ok(manifest)
    }

    /// Validate a plugin manifest
    fn validate_manifest(&self, manifest: &PluginManifest) -> Result<()> {
        // Check ID format (should be reverse-DNS)
        if !manifest.id.contains('.') {
            return Err(AgentError::Validation(
                "Plugin ID must use reverse-DNS format (e.g., com.example.plugin)".to_string(),
            ));
        }

        // Check version format (should be semver)
        if !manifest.version.chars().any(|c| c == '.') {
            return Err(AgentError::Validation(
                "Plugin version must be in semver format (e.g., 1.0.0)".to_string(),
            ));
        }

        // Check description length
        if manifest.description.len() > 200 {
            return Err(AgentError::Validation(
                "Plugin description must be 200 characters or less".to_string(),
            ));
        }

        // Check capabilities not empty
        if manifest.capabilities.is_empty() {
            return Err(AgentError::Validation(
                "Plugin must declare at least one capability".to_string(),
            ));
        }

        Ok(())
    }

    /// Get all plugins with their enabled state
    pub fn list_plugins(&self) -> Result<Vec<(PluginManifest, bool)>> {
        let manifests = self.discover_plugins()?;
        let mut plugins = Vec::new();

        for manifest in manifests {
            let enabled = self.registry
                .get(&manifest.id)
                .map(|entry| entry.enabled)
                .unwrap_or(false);
            plugins.push((manifest, enabled));
        }

        Ok(plugins)
    }

    /// Enable a plugin
    pub fn enable_plugin(&mut self, plugin_id: &str) -> Result<()> {
        // Verify plugin exists
        let manifests = self.discover_plugins()?;
        let manifest = manifests
            .iter()
            .find(|m| m.id == plugin_id)
            .ok_or_else(|| AgentError::NotFound(format!("Plugin not found: {}", plugin_id)))?;

        // Add or update registry entry
        self.registry.insert(
            plugin_id.to_string(),
            PluginRegistryEntry {
                id: plugin_id.to_string(),
                enabled: true,
                installed_at: chrono::Utc::now(),
                version: manifest.version.clone(),
            },
        );

        self.save_registry()?;
        Ok(())
    }

    /// Disable a plugin
    pub fn disable_plugin(&mut self, plugin_id: &str) -> Result<()> {
        if let Some(entry) = self.registry.get_mut(plugin_id) {
            entry.enabled = false;
            self.save_registry()?;
        }
        Ok(())
    }

    /// Uninstall a plugin (remove from disk)
    pub fn uninstall_plugin(&mut self, plugin_id: &str) -> Result<()> {
        // Remove registry entry
        self.registry.remove(plugin_id);
        self.save_registry()?;

        // Remove plugin directory
        let plugin_dir = self.plugins_dir.join(plugin_id);
        if plugin_dir.exists() {
            fs::remove_dir_all(&plugin_dir)
                .map_err(|e| AgentError::FileSystem(format!("Failed to remove plugin dir: {}", e)))?;
        }

        Ok(())
    }

    /// Save registry to disk
    fn save_registry(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.registry)
            .map_err(|e| AgentError::Serialization(format!("Failed to serialize registry: {}", e)))?;

        fs::write(&self.registry_path, json)
            .map_err(|e| AgentError::FileSystem(format!("Failed to write registry: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_manifest() -> PluginManifest {
        PluginManifest {
            id: "com.example.test-plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            author: "Test Author".to_string(),
            description: "A test plugin".to_string(),
            capabilities: vec![PluginCapability::DataSource],
            permissions: vec![PluginPermission::ReadFiles],
            entry_point: "plugin.wasm".to_string(),
            icon: None,
            homepage: None,
        }
    }

    #[test]
    fn test_manifest_validation() {
        let manager = PluginManager::new().unwrap();
        let manifest = create_test_manifest();
        assert!(manager.validate_manifest(&manifest).is_ok());
    }

    #[test]
    fn test_invalid_plugin_id() {
        let manager = PluginManager::new().unwrap();
        let mut manifest = create_test_manifest();
        manifest.id = "invalid-id".to_string(); // Missing reverse-DNS format
        assert!(manager.validate_manifest(&manifest).is_err());
    }

    #[test]
    fn test_invalid_version() {
        let manager = PluginManager::new().unwrap();
        let mut manifest = create_test_manifest();
        manifest.version = "1".to_string(); // Not semver
        assert!(manager.validate_manifest(&manifest).is_err());
    }

    #[test]
    fn test_description_too_long() {
        let manager = PluginManager::new().unwrap();
        let mut manifest = create_test_manifest();
        manifest.description = "a".repeat(201);
        assert!(manager.validate_manifest(&manifest).is_err());
    }

    #[test]
    fn test_no_capabilities() {
        let manager = PluginManager::new().unwrap();
        let mut manifest = create_test_manifest();
        manifest.capabilities = vec![];
        assert!(manager.validate_manifest(&manifest).is_err());
    }
}
