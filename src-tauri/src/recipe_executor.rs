// SQL Recipe Executor
//
// Executes pre-built SQL analysis templates with user-provided parameters.
// Validates required tables/columns exist before running queries.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::{AgentError, Result};

/// SQL Recipe - pre-built analysis template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub description: String,
    pub data_source: String,
    pub required_tables: Vec<RequiredTable>,
    pub sql_template: String,
    #[serde(default)]
    pub parameters: Vec<RecipeParameter>,
    pub tier: RecipeTier,
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub author: String,
    pub version: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub metrics: RecipeMetrics,
    pub example_output: Option<String>,
    pub changelog_url: Option<String>,
    pub documentation_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredTable {
    pub name: String,
    #[serde(default)]
    pub required_columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: ParameterType,
    pub label: Option<String>,
    pub description: Option<String>,
    pub default: Option<serde_json::Value>,
    pub validation: Option<ParameterValidation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    Date,
    Int,
    Float,
    String,
    Boolean,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterValidation {
    pub min: Option<serde_json::Value>,
    pub max: Option<serde_json::Value>,
    pub pattern: Option<String>,
    pub options: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RecipeTier {
    Free,
    Pro,
    Team,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecipeMetrics {
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub runs: u64,
    #[serde(default)]
    pub rating: f32,
    #[serde(default)]
    pub review_count: u32,
}

/// Recipe execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeResult {
    pub recipe_id: String,
    pub recipe_name: String,
    pub executed_sql: String,
    pub parameters: HashMap<String, serde_json::Value>,
    pub row_count: usize,
    pub columns: Vec<String>,
    pub rows: Vec<HashMap<String, serde_json::Value>>,
    pub execution_time_ms: u64,
}

/// Recipe executor - validates and runs SQL recipes
pub struct RecipeExecutor {
    recipes: HashMap<String, Recipe>,
}

impl RecipeExecutor {
    /// Create new executor
    pub fn new() -> Self {
        Self {
            recipes: HashMap::new(),
        }
    }

    /// Load recipe from JSON file
    pub fn load_recipe(&mut self, path: &Path) -> Result<Recipe> {
        let content = fs::read_to_string(path)
            .map_err(|e| AgentError::FileSystem(format!("Failed to read recipe: {}", e)))?;

        let recipe: Recipe = serde_json::from_str(&content)
            .map_err(|e| AgentError::Serialization(format!("Invalid recipe JSON: {}", e)))?;

        // Validate recipe
        self.validate_recipe(&recipe)?;

        // Store in cache
        self.recipes.insert(recipe.id.clone(), recipe.clone());

        Ok(recipe)
    }

    /// Load all recipes from directory
    pub fn load_recipes_from_dir(&mut self, dir: &Path) -> Result<Vec<Recipe>> {
        let mut loaded = Vec::new();

        if !dir.exists() {
            return Ok(loaded);
        }

        let entries = fs::read_dir(dir)
            .map_err(|e| AgentError::FileSystem(format!("Failed to read recipes directory: {}", e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| AgentError::FileSystem(format!("Failed to read entry: {}", e)))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                match self.load_recipe(&path) {
                    Ok(recipe) => loaded.push(recipe),
                    Err(e) => {
                        // Log error but continue loading other recipes
                        eprintln!("Failed to load recipe {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(loaded)
    }

    /// Get recipe by ID
    pub fn get_recipe(&self, recipe_id: &str) -> Option<&Recipe> {
        self.recipes.get(recipe_id)
    }

    /// List all loaded recipes
    pub fn list_recipes(&self) -> Vec<&Recipe> {
        self.recipes.values().collect()
    }

    /// Search recipes by query (name, description, tags)
    pub fn search_recipes(&self, query: &str) -> Vec<&Recipe> {
        let query_lower = query.to_lowercase();

        self.recipes
            .values()
            .filter(|recipe| {
                recipe.name.to_lowercase().contains(&query_lower)
                    || recipe.description.to_lowercase().contains(&query_lower)
                    || recipe.data_source.to_lowercase().contains(&query_lower)
                    || recipe.tags.iter().any(|tag| tag.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    /// Filter recipes by data source
    pub fn filter_by_data_source(&self, data_source: &str) -> Vec<&Recipe> {
        self.recipes
            .values()
            .filter(|recipe| recipe.data_source.eq_ignore_ascii_case(data_source))
            .collect()
    }

    /// Filter recipes by tier
    pub fn filter_by_tier(&self, tier: RecipeTier) -> Vec<&Recipe> {
        self.recipes
            .values()
            .filter(|recipe| matches!((tier, &recipe.tier),
                (RecipeTier::Free, RecipeTier::Free) |
                (RecipeTier::Pro, RecipeTier::Pro) |
                (RecipeTier::Team, RecipeTier::Team)))
            .collect()
    }

    /// Validate recipe structure
    fn validate_recipe(&self, recipe: &Recipe) -> Result<()> {
        // Validate ID format (reverse-DNS)
        if !recipe.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-') {
            return Err(AgentError::Validation(format!(
                "Recipe ID must be lowercase alphanumeric with dots/dashes: {}",
                recipe.id
            )));
        }

        // Validate required fields
        if recipe.name.is_empty() {
            return Err(AgentError::Validation("Recipe name cannot be empty".to_string()));
        }

        if recipe.sql_template.is_empty() {
            return Err(AgentError::Validation("Recipe SQL template cannot be empty".to_string()));
        }

        if recipe.required_tables.is_empty() {
            return Err(AgentError::Validation("Recipe must require at least one table".to_string()));
        }

        // Validate parameter placeholders match defined parameters
        let defined_params: Vec<&str> = recipe.parameters.iter().map(|p| p.name.as_str()).collect();
        let template_params = self.extract_placeholders(&recipe.sql_template);

        for template_param in &template_params {
            if !defined_params.contains(&template_param.as_str()) {
                return Err(AgentError::Validation(format!(
                    "SQL template uses undefined parameter: {{{{{}}}}}",
                    template_param
                )));
            }
        }

        Ok(())
    }

    /// Extract {{parameter}} placeholders from SQL template
    fn extract_placeholders(&self, template: &str) -> Vec<String> {
        let mut placeholders = Vec::new();
        let mut chars = template.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next(); // consume second {
                    let mut param = String::new();

                    while let Some(c) = chars.next() {
                        if c == '}' {
                            if chars.peek() == Some(&'}') {
                                chars.next(); // consume second }
                                placeholders.push(param);
                                break;
                            }
                        } else {
                            param.push(c);
                        }
                    }
                }
            }
        }

        placeholders
    }

    /// Render SQL template with parameters
    pub fn render_sql(
        &self,
        recipe: &Recipe,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<String> {
        let mut sql = recipe.sql_template.clone();

        // Validate all required parameters are provided
        for param in &recipe.parameters {
            if param.default.is_none() && !params.contains_key(&param.name) {
                return Err(AgentError::Validation(format!(
                    "Missing required parameter: {}",
                    param.name
                )));
            }
        }

        // Replace placeholders
        for (key, value) in params {
            let placeholder = format!("{{{{{}}}}}", key);
            let value_str = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => value.to_string(),
            };

            sql = sql.replace(&placeholder, &value_str);
        }

        // Replace any remaining placeholders with defaults
        for param in &recipe.parameters {
            let placeholder = format!("{{{{{}}}}}", param.name);
            if sql.contains(&placeholder) {
                if let Some(default) = &param.default {
                    let default_str = match default {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        _ => default.to_string(),
                    };
                    sql = sql.replace(&placeholder, &default_str);
                }
            }
        }

        Ok(sql)
    }

    /// Validate that required tables and columns exist
    /// This would be called before execution with actual table metadata
    pub fn validate_tables(
        &self,
        recipe: &Recipe,
        available_tables: &HashMap<String, Vec<String>>, // table_name -> column_names
    ) -> Result<()> {
        for required in &recipe.required_tables {
            // Check if table exists (case-insensitive)
            let table_exists = available_tables
                .keys()
                .any(|name| name.eq_ignore_ascii_case(&required.name));

            if !table_exists {
                return Err(AgentError::Validation(format!(
                    "Required table '{}' not found in dataset",
                    required.name
                )));
            }

            // Get actual table name (with correct casing)
            let actual_table_name = available_tables
                .keys()
                .find(|name| name.eq_ignore_ascii_case(&required.name))
                .unwrap();

            // Check required columns
            if !required.required_columns.is_empty() {
                let available_columns = &available_tables[actual_table_name];

                for required_col in &required.required_columns {
                    let col_exists = available_columns
                        .iter()
                        .any(|col| col.eq_ignore_ascii_case(required_col));

                    if !col_exists {
                        return Err(AgentError::Validation(format!(
                            "Required column '{}' not found in table '{}'",
                            required_col, required.name
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_placeholders() {
        let executor = RecipeExecutor::new();
        let template = "SELECT * FROM orders WHERE date >= '{{start_date}}' AND amount > {{min_amount}}";
        let placeholders = executor.extract_placeholders(template);

        assert_eq!(placeholders.len(), 2);
        assert!(placeholders.contains(&"start_date".to_string()));
        assert!(placeholders.contains(&"min_amount".to_string()));
    }

    #[test]
    fn test_render_sql() {
        let mut executor = RecipeExecutor::new();
        let recipe = Recipe {
            id: "test.recipe".to_string(),
            name: "Test Recipe".to_string(),
            description: "Test".to_string(),
            data_source: "Generic".to_string(),
            required_tables: vec![RequiredTable {
                name: "orders".to_string(),
                required_columns: vec![],
            }],
            sql_template: "SELECT * FROM orders WHERE date >= '{{start_date}}' LIMIT {{limit}}".to_string(),
            parameters: vec![
                RecipeParameter {
                    name: "start_date".to_string(),
                    param_type: ParameterType::Date,
                    label: None,
                    description: None,
                    default: Some(serde_json::Value::String("2024-01-01".to_string())),
                    validation: None,
                },
                RecipeParameter {
                    name: "limit".to_string(),
                    param_type: ParameterType::Int,
                    label: None,
                    description: None,
                    default: Some(serde_json::Value::Number(100.into())),
                    validation: None,
                },
            ],
            tier: RecipeTier::Free,
            category: None,
            tags: vec![],
            author: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: None,
            updated_at: None,
            metrics: RecipeMetrics::default(),
            example_output: None,
            changelog_url: None,
            documentation_url: None,
        };

        executor.recipes.insert(recipe.id.clone(), recipe.clone());

        let mut params = HashMap::new();
        params.insert("start_date".to_string(), serde_json::Value::String("2024-06-01".to_string()));
        params.insert("limit".to_string(), serde_json::Value::Number(50.into()));

        let sql = executor.render_sql(&recipe, &params).unwrap();
        assert_eq!(sql, "SELECT * FROM orders WHERE date >= '2024-06-01' LIMIT 50");
    }

    #[test]
    fn test_validate_tables() {
        let executor = RecipeExecutor::new();
        let recipe = Recipe {
            id: "test.recipe".to_string(),
            name: "Test Recipe".to_string(),
            description: "Test".to_string(),
            data_source: "Generic".to_string(),
            required_tables: vec![RequiredTable {
                name: "orders".to_string(),
                required_columns: vec!["customer_id".to_string(), "total".to_string()],
            }],
            sql_template: "SELECT * FROM orders".to_string(),
            parameters: vec![],
            tier: RecipeTier::Free,
            category: None,
            tags: vec![],
            author: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: None,
            updated_at: None,
            metrics: RecipeMetrics::default(),
            example_output: None,
            changelog_url: None,
            documentation_url: None,
        };

        let mut available_tables = HashMap::new();
        available_tables.insert(
            "orders".to_string(),
            vec!["customer_id".to_string(), "total".to_string(), "date".to_string()],
        );

        // Should pass - all required tables and columns exist
        assert!(executor.validate_tables(&recipe, &available_tables).is_ok());

        // Should fail - missing column
        let mut missing_col_tables = HashMap::new();
        missing_col_tables.insert("orders".to_string(), vec!["customer_id".to_string()]);
        assert!(executor.validate_tables(&recipe, &missing_col_tables).is_err());

        // Should fail - missing table
        let empty_tables = HashMap::new();
        assert!(executor.validate_tables(&recipe, &empty_tables).is_err());
    }
}
