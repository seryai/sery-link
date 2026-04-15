//! Plugin runtime - WebAssembly execution layer for Sery Link plugins
//!
//! This module provides safe, sandboxed execution of plugin code via WebAssembly.
//! Plugins can declare capabilities and permissions, and the runtime enforces
//! access control based on those declarations.
//!
//! Architecture:
//! - Plugins are compiled to WebAssembly (.wasm files)
//! - The runtime loads WASM modules using wasmer
//! - Host functions are exposed based on plugin permissions
//! - Memory is isolated per plugin instance
//! - Execution is sandboxed (no direct filesystem/network access)

use crate::error::{AgentError, Result};
use crate::plugin::{PluginManifest, PluginPermission};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use wasmer::{imports, Function, Instance, Memory, Module, Store, Value};

/// Global registry mapping plugin IDs to their WASM memory
/// This allows host functions to access memory for string reading/writing
static PLUGIN_MEMORY: Lazy<Arc<RwLock<HashMap<String, Memory>>>> =
    Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

/// Environment passed to host functions
/// Contains sandboxing constraints and shared state
#[derive(Clone)]
struct HostEnv {
    /// Allowed file paths (for read_file sandboxing)
    /// Plugins with ReadFiles permission can only read from these paths
    allowed_paths: Arc<Mutex<Vec<PathBuf>>>,
    /// Current plugin ID (for memory access from host functions)
    plugin_id: Arc<Mutex<Option<String>>>,
}

impl HostEnv {
    fn new() -> Self {
        Self {
            allowed_paths: Arc::new(Mutex::new(vec![])),
            plugin_id: Arc::new(Mutex::new(None)),
        }
    }

    fn set_plugin_id(&self, id: String) {
        let mut plugin_id = self.plugin_id.lock().unwrap();
        *plugin_id = Some(id);
    }

    fn get_plugin_id(&self) -> Option<String> {
        let plugin_id = self.plugin_id.lock().unwrap();
        plugin_id.clone()
    }

    /// Check if a path is allowed to be read
    fn is_path_allowed(&self, path: &Path) -> bool {
        let paths = self.allowed_paths.lock().unwrap();
        if paths.is_empty() {
            return false; // No paths allowed by default
        }

        // Check if path is under any allowed directory
        for allowed in paths.iter() {
            if path.starts_with(allowed) {
                return true;
            }
        }
        false
    }

    /// Read a file if the path is allowed
    /// Returns file contents as bytes, or error
    fn read_file_if_allowed(&self, path: &Path) -> std::result::Result<Vec<u8>, String> {
        if !self.is_path_allowed(path) {
            return Err(format!("Access denied: {} is not in allowed paths", path.display()));
        }

        std::fs::read(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))
    }
}

/// Plugin runtime manages WebAssembly plugin execution
pub struct PluginRuntime {
    store: Store,
    instances: HashMap<String, PluginInstance>,
    env: HostEnv,
}

/// A loaded plugin instance
struct PluginInstance {
    instance: Instance,
    manifest: PluginManifest,
}

impl PluginRuntime {
    /// Create a new plugin runtime
    pub fn new() -> Self {
        Self {
            store: Store::default(),
            instances: HashMap::new(),
            env: HostEnv::new(),
        }
    }

    /// Set allowed file paths for plugins with ReadFiles permission
    /// Plugins can only read files under these directories
    pub fn set_allowed_paths(&mut self, paths: Vec<PathBuf>) {
        let mut allowed = self.env.allowed_paths.lock().unwrap();
        *allowed = paths;
    }

    /// Load a plugin from disk
    pub fn load_plugin(&mut self, plugin_dir: &Path, manifest: PluginManifest) -> Result<()> {
        // Read the WASM binary
        let wasm_path = plugin_dir.join(&manifest.entry_point);
        let wasm_bytes = fs::read(&wasm_path).map_err(|e| {
            AgentError::FileSystem(format!("Failed to read WASM file: {}", e))
        })?;

        // Compile the module
        let module = Module::new(&self.store, wasm_bytes).map_err(|e| {
            AgentError::Validation(format!("Failed to compile WASM module: {}", e))
        })?;

        // Set the plugin ID in HostEnv so host functions know which plugin is calling
        self.env.set_plugin_id(manifest.id.clone());

        // Build imports based on permissions
        let import_object = self.build_imports(&manifest);

        // Instantiate the module
        let instance = Instance::new(&mut self.store, &module, &import_object).map_err(|e| {
            AgentError::Validation(format!("Failed to instantiate WASM module: {}", e))
        })?;

        // Store the memory in the global registry so host functions can access it
        if let Ok(memory) = instance.exports.get_memory("memory") {
            let mut registry = PLUGIN_MEMORY.write().unwrap();
            registry.insert(manifest.id.clone(), memory.clone());
        }

        // Call _initialize if it exists
        if let Ok(init_fn) = instance.exports.get_function("_initialize") {
            let _ = init_fn.call(&mut self.store, &[]);
        }

        // Store the instance
        self.instances.insert(
            manifest.id.clone(),
            PluginInstance { instance, manifest },
        );

        Ok(())
    }

    /// Read a file through a plugin (with sandboxing)
    /// This is called by the host, not directly by WASM
    pub fn read_file_for_plugin(&mut self, plugin_id: &str, file_path: &str) -> Result<Vec<u8>> {
        let path = PathBuf::from(file_path);

        self.env.read_file_if_allowed(&path)
            .map_err(|e| AgentError::FileSystem(e))
    }

    /// Write a string to WASM memory (allocates memory in plugin)
    /// Returns pointer to the string in WASM memory
    fn write_string_to_memory(&mut self, plugin_id: &str, s: &str) -> Result<i32> {
        let instance_data = self.instances.get(plugin_id)
            .ok_or_else(|| AgentError::NotFound(format!("Plugin not loaded: {}", plugin_id)))?;

        // Get the memory export
        let memory = instance_data.instance.exports.get_memory("memory")
            .map_err(|e| AgentError::Validation(format!("Plugin missing memory export: {}", e)))?;

        // Allocate memory in the plugin (if plugin exports alloc function)
        let alloc_fn = instance_data.instance.exports.get_function("alloc").ok();

        let ptr = if let Some(alloc) = alloc_fn {
            // Call plugin's alloc function
            let len = s.len() as i32;
            let result = alloc.call(&mut self.store, &[Value::I32(len)])
                .map_err(|e| AgentError::Validation(format!("Failed to allocate memory: {}", e)))?;

            if let Some(Value::I32(p)) = result.first() {
                *p
            } else {
                return Err(AgentError::Validation("alloc didn't return i32".to_string()));
            }
        } else {
            // Fallback: use a fixed offset (simple but limited)
            1024 // Start after first 1KB reserved for plugin stack
        };

        // Write string bytes to WASM memory
        let view = memory.view(&self.store);
        let bytes = s.as_bytes();
        for (i, byte) in bytes.iter().enumerate() {
            view.write_u8((ptr as u64) + (i as u64), *byte)
                .map_err(|e| AgentError::Validation(format!("Failed to write to memory: {}", e)))?;
        }

        Ok(ptr)
    }

    /// Read a string from WASM memory
    fn read_string_from_memory(&self, plugin_id: &str, ptr: i32, len: i32) -> Result<String> {
        let instance_data = self.instances.get(plugin_id)
            .ok_or_else(|| AgentError::NotFound(format!("Plugin not loaded: {}", plugin_id)))?;

        let memory = instance_data.instance.exports.get_memory("memory")
            .map_err(|e| AgentError::Validation(format!("Plugin missing memory export: {}", e)))?;

        let view = memory.view(&self.store);
        let mut bytes = vec![0u8; len as usize];

        for i in 0..len as usize {
            bytes[i] = view.read_u8((ptr as u64) + (i as u64))
                .map_err(|e| AgentError::Validation(format!("Failed to read from memory: {}", e)))?;
        }

        String::from_utf8(bytes)
            .map_err(|e| AgentError::Validation(format!("Invalid UTF-8: {}", e)))
    }

    /// Build host function imports based on plugin permissions
    fn build_imports(&mut self, manifest: &PluginManifest) -> wasmer::Imports {
        let mut imports = imports! {};

        // Always provide basic logging
        let log_fn = Function::new_typed(&mut self.store, |msg: i32| {
            println!("[plugin] log: {}", msg);
        });
        imports.define("env", "log", log_fn);

        // Permission-based imports
        for permission in &manifest.permissions {
            match permission {
                PluginPermission::ReadFiles => {
                    // Expose file reading function (stub for Phase 4)
                    // Phase 4: Full WASM-callable read_file requires wasmer FunctionEnv pattern
                    // For now, use execute_plugin_with_file command (host reads, passes to plugin)
                    let read_file_fn = Function::new_typed(
                        &mut self.store,
                        |_path_ptr: i32, _path_len: i32, _output_ptr: i32, _output_max_len: i32| -> i32 {
                            // TODO Phase 5: Implement with FunctionEnvMut for Store access
                            // Signature: read_file(path_ptr, path_len, output_ptr, output_max) -> bytes or -1
                            // Current workaround: use execute_plugin_with_file Tauri command
                            -1 // Not yet implemented
                        },
                    );
                    imports.define("env", "read_file", read_file_fn);
                }
                PluginPermission::Network => {
                    // Expose HTTP functions
                    let http_get_fn = Function::new_typed(&mut self.store, |url: i32| -> i32 {
                        // TODO: Implement sandboxed HTTP
                        -1
                    });
                    imports.define("env", "http_get", http_get_fn);
                }
                PluginPermission::ExecuteCommands => {
                    // Expose command execution (highly restricted)
                    let exec_fn = Function::new_typed(&mut self.store, |cmd: i32| -> i32 {
                        // TODO: Implement sandboxed command execution
                        -1
                    });
                    imports.define("env", "exec", exec_fn);
                }
                PluginPermission::Clipboard => {
                    // Expose clipboard functions
                    let get_clipboard_fn = Function::new_typed(&mut self.store, || -> i32 {
                        // TODO: Implement clipboard access
                        -1
                    });
                    imports.define("env", "get_clipboard", get_clipboard_fn);
                }
            }
        }

        imports
    }

    /// Execute a plugin function
    pub fn execute(
        &mut self,
        plugin_id: &str,
        function_name: &str,
        args: Vec<Value>,
    ) -> Result<Vec<Value>> {
        let instance_data = self
            .instances
            .get(plugin_id)
            .ok_or_else(|| AgentError::NotFound(format!("Plugin not loaded: {}", plugin_id)))?;

        // Get the exported function
        let function = instance_data
            .instance
            .exports
            .get_function(function_name)
            .map_err(|e| {
                AgentError::NotFound(format!("Function '{}' not found: {}", function_name, e))
            })?;

        // Call the function
        let result = function.call(&mut self.store, &args).map_err(|e| {
            AgentError::Validation(format!("Plugin execution failed: {}", e))
        })?;

        Ok(result.to_vec())
    }

    /// Unload a plugin
    pub fn unload_plugin(&mut self, plugin_id: &str) -> Result<()> {
        self.instances
            .remove(plugin_id)
            .ok_or_else(|| AgentError::NotFound(format!("Plugin not loaded: {}", plugin_id)))?;
        Ok(())
    }

    /// Check if a plugin is loaded
    pub fn is_loaded(&self, plugin_id: &str) -> bool {
        self.instances.contains_key(plugin_id)
    }

    /// Get list of loaded plugin IDs
    pub fn loaded_plugins(&self) -> Vec<String> {
        self.instances.keys().cloned().collect()
    }
}

/// Helper for data source plugins (parse file format)
pub fn execute_data_source_plugin(
    runtime: &mut PluginRuntime,
    plugin_id: &str,
    file_path: &str,
    file_bytes: &[u8],
) -> Result<String> {
    // For Phase 1, we return a placeholder
    // In a real implementation, we'd:
    // 1. Write file_bytes to plugin memory
    // 2. Call the plugin's `parse` function
    // 3. Read the result from plugin memory
    // 4. Return as JSON string

    if !runtime.is_loaded(plugin_id) {
        return Err(AgentError::NotFound(format!(
            "Plugin not loaded: {}",
            plugin_id
        )));
    }

    // TODO: Implement actual plugin execution
    // For now, return a stub response
    Ok(format!(
        "{{\"plugin\":\"{}\",\"file\":\"{}\",\"size\":{}}}",
        plugin_id,
        file_path,
        file_bytes.len()
    ))
}

/// Helper for transform plugins (data transformation)
pub fn execute_transform_plugin(
    runtime: &mut PluginRuntime,
    plugin_id: &str,
    input_data: &str,
) -> Result<String> {
    if !runtime.is_loaded(plugin_id) {
        return Err(AgentError::NotFound(format!(
            "Plugin not loaded: {}",
            plugin_id
        )));
    }

    // TODO: Implement actual plugin execution
    Ok(format!("{{\"transformed\":true,\"plugin\":\"{}\"}}", plugin_id))
}

/// Helper for viewer plugins (render data to HTML/JSON)
pub fn execute_viewer_plugin(
    runtime: &mut PluginRuntime,
    plugin_id: &str,
    data: &str,
) -> Result<String> {
    if !runtime.is_loaded(plugin_id) {
        return Err(AgentError::NotFound(format!(
            "Plugin not loaded: {}",
            plugin_id
        )));
    }

    // TODO: Implement actual plugin execution
    Ok(format!("<div>Rendered by {}</div>", plugin_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginCapability;

    fn test_manifest() -> PluginManifest {
        PluginManifest {
            id: "com.test.plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            author: "Test".to_string(),
            description: "Test plugin".to_string(),
            capabilities: vec![PluginCapability::DataSource],
            permissions: vec![PluginPermission::ReadFiles],
            entry_point: "plugin.wasm".to_string(),
            icon: None,
            homepage: None,
        }
    }

    #[test]
    fn test_runtime_creation() {
        let runtime = PluginRuntime::new();
        assert_eq!(runtime.loaded_plugins().len(), 0);
    }

    #[test]
    fn test_is_loaded() {
        let runtime = PluginRuntime::new();
        assert!(!runtime.is_loaded("com.test.plugin"));
    }

    #[test]
    fn test_unload_nonexistent() {
        let mut runtime = PluginRuntime::new();
        assert!(runtime.unload_plugin("nonexistent").is_err());
    }

    #[test]
    #[ignore] // Requires example plugin to be present
    fn test_load_and_execute_hello_world() {
        let mut runtime = PluginRuntime::new();

        // Path to the example hello-world plugin
        let plugin_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../examples/plugins/hello-world");

        // Skip test if plugin doesn't exist
        if !plugin_dir.exists() {
            println!("Skipping test - example plugin not found at {:?}", plugin_dir);
            return;
        }

        // Load the plugin manifest
        let manifest_path = plugin_dir.join("plugin.json");
        let manifest_str = std::fs::read_to_string(&manifest_path)
            .expect("Failed to read plugin manifest");
        let manifest: PluginManifest = serde_json::from_str(&manifest_str)
            .expect("Failed to parse plugin manifest");

        // Load the plugin into the runtime
        runtime.load_plugin(&plugin_dir, manifest.clone())
            .expect("Failed to load plugin");

        // Verify the plugin is loaded
        assert!(runtime.is_loaded(&manifest.id));
        assert_eq!(runtime.loaded_plugins().len(), 1);

        // Execute the greet function
        let result = runtime.execute(&manifest.id, "greet", vec![])
            .expect("Failed to execute greet function");

        // The greet function should return 42
        assert_eq!(result.len(), 1);
        if let Value::I32(val) = result[0] {
            assert_eq!(val, 42);
        } else {
            panic!("Expected I32 return value");
        }

        // Unload the plugin
        runtime.unload_plugin(&manifest.id)
            .expect("Failed to unload plugin");
        assert!(!runtime.is_loaded(&manifest.id));
    }

    #[test]
    #[ignore] // Requires CSV parser plugin to be present
    fn test_csv_parser_plugin() {
        let mut runtime = PluginRuntime::new();

        // Path to the CSV parser plugin
        let plugin_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../examples/plugins/csv-parser");

        // Skip test if plugin doesn't exist
        if !plugin_dir.exists() {
            println!("Skipping test - CSV parser plugin not found at {:?}", plugin_dir);
            return;
        }

        // Load the plugin manifest
        let manifest_path = plugin_dir.join("plugin.json");
        let manifest_str = std::fs::read_to_string(&manifest_path)
            .expect("Failed to read plugin manifest");
        let manifest: PluginManifest = serde_json::from_str(&manifest_str)
            .expect("Failed to parse plugin manifest");

        // Load the plugin into the runtime
        runtime.load_plugin(&plugin_dir, manifest.clone())
            .expect("Failed to load CSV parser plugin");

        // Verify the plugin is loaded
        assert!(runtime.is_loaded(&manifest.id));

        // Test get_column_count - should return 3 (name, age, city)
        let result = runtime.execute(&manifest.id, "get_column_count", vec![])
            .expect("Failed to execute get_column_count");
        assert_eq!(result.len(), 1);
        if let Value::I32(count) = result[0] {
            assert_eq!(count, 3, "Expected 3 columns");
        } else {
            panic!("Expected I32 return value");
        }

        // Test get_row_count - should return 3 (data rows)
        let result = runtime.execute(&manifest.id, "get_row_count", vec![])
            .expect("Failed to execute get_row_count");
        if let Value::I32(count) = result[0] {
            assert_eq!(count, 3, "Expected 3 rows");
        } else {
            panic!("Expected I32 return value");
        }

        // Test validate_csv - should return 1 (valid)
        let result = runtime.execute(&manifest.id, "validate_csv", vec![])
            .expect("Failed to execute validate_csv");
        if let Value::I32(valid) = result[0] {
            assert_eq!(valid, 1, "Expected CSV to be valid");
        } else {
            panic!("Expected I32 return value");
        }

        // Test get_version - should return 1000 (version 1.0)
        let result = runtime.execute(&manifest.id, "get_version", vec![])
            .expect("Failed to execute get_version");
        if let Value::I32(version) = result[0] {
            assert_eq!(version, 1000, "Expected version 1.0 (1000)");
        } else {
            panic!("Expected I32 return value");
        }

        // Unload the plugin
        runtime.unload_plugin(&manifest.id)
            .expect("Failed to unload plugin");
        assert!(!runtime.is_loaded(&manifest.id));

        println!("CSV parser plugin test passed!");
        println!("  - Column count: 3 ✓");
        println!("  - Row count: 3 ✓");
        println!("  - CSV valid: true ✓");
        println!("  - Version: 1.0 ✓");
    }

    #[test]
    fn test_file_reading_sandboxing() {
        let mut runtime = PluginRuntime::new();

        // Create a temporary test file
        let test_dir = std::env::temp_dir().join("sery_plugin_test");
        std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

        let allowed_file = test_dir.join("allowed.txt");
        std::fs::write(&allowed_file, b"Hello from allowed file")
            .expect("Failed to write test file");

        let denied_dir = std::env::temp_dir().join("sery_plugin_denied");
        std::fs::create_dir_all(&denied_dir).expect("Failed to create denied dir");
        let denied_file = denied_dir.join("denied.txt");
        std::fs::write(&denied_file, b"This should not be readable")
            .expect("Failed to write denied file");

        // Set allowed paths - only test_dir is allowed
        runtime.set_allowed_paths(vec![test_dir.clone()]);

        // Test 1: Reading from allowed path should succeed
        let result = runtime.read_file_for_plugin(
            "com.test.plugin",
            allowed_file.to_str().unwrap()
        );
        assert!(result.is_ok(), "Should be able to read from allowed path");
        let content = result.unwrap();
        assert_eq!(content, b"Hello from allowed file");

        // Test 2: Reading from denied path should fail
        let result = runtime.read_file_for_plugin(
            "com.test.plugin",
            denied_file.to_str().unwrap()
        );
        assert!(result.is_err(), "Should NOT be able to read from denied path");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Access denied"), "Error should mention access denied");

        // Clean up
        std::fs::remove_dir_all(&test_dir).ok();
        std::fs::remove_dir_all(&denied_dir).ok();

        println!("File reading sandboxing test passed!");
        println!("  - Allowed path readable ✓");
        println!("  - Denied path blocked ✓");
    }
}
