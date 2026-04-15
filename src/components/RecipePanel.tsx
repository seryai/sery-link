import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Search, Filter, Database, TrendingUp, Users, DollarSign, BarChart3, FileSpreadsheet, Award } from 'lucide-react';

interface Recipe {
  id: string;
  name: string;
  description: string;
  data_source: string;
  tier: 'FREE' | 'PRO' | 'TEAM';
  category: string | null;
  tags: string[];
  author: string;
  version: string;
  metrics: {
    downloads: number;
    runs: number;
    rating: number;
    review_count: number;
  };
  required_tables: Array<{
    name: string;
    required_columns: string[];
  }>;
  parameters: Array<{
    name: string;
    type: string;
    label: string | null;
    description: string | null;
    default: any;
  }>;
}

type ViewMode = 'all' | 'free' | 'pro';
type DataSource = 'all' | 'Shopify' | 'Stripe' | 'Google Analytics' | 'CSV' | 'Generic';

export function RecipePanel() {
  const [recipes, setRecipes] = useState<Recipe[]>([]);
  const [filteredRecipes, setFilteredRecipes] = useState<Recipe[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [viewMode, setViewMode] = useState<ViewMode>('all');
  const [dataSource, setDataSource] = useState<DataSource>('all');
  const [selectedRecipe, setSelectedRecipe] = useState<Recipe | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Load recipes on mount
  useEffect(() => {
    loadRecipes();
  }, []);

  // Filter recipes when search/filters change
  useEffect(() => {
    filterRecipes();
  }, [searchQuery, viewMode, dataSource, recipes]);

  const loadRecipes = async () => {
    try {
      setLoading(true);
      setError(null);

      // Load recipes from examples directory (bundled with app)
      // In production, this would load from ~/.sery/recipes/
      const recipesDir = 'examples/recipes';
      const loaded = await invoke<Recipe[]>('load_recipes_from_dir', { dirPath: recipesDir });

      setRecipes(loaded);
    } catch (err) {
      console.error('Failed to load recipes:', err);
      setError(err instanceof Error ? err.message : String(err));
      setRecipes([]);
    } finally {
      setLoading(false);
    }
  };

  const filterRecipes = async () => {
    try {
      let filtered = [...recipes];

      // Filter by tier
      if (viewMode === 'free') {
        filtered = filtered.filter(r => r.tier === 'FREE');
      } else if (viewMode === 'pro') {
        filtered = filtered.filter(r => r.tier === 'PRO' || r.tier === 'TEAM');
      }

      // Filter by data source
      if (dataSource !== 'all') {
        filtered = await invoke<Recipe[]>('filter_recipes_by_data_source', {
          dataSource
        });
      }

      // Search filter
      if (searchQuery.trim()) {
        const results = await invoke<Recipe[]>('search_recipes', {
          query: searchQuery
        });
        const resultIds = new Set(results.map(r => r.id));
        filtered = filtered.filter(r => resultIds.has(r.id));
      }

      setFilteredRecipes(filtered);
    } catch (err) {
      console.error('Filter error:', err);
      setFilteredRecipes(recipes);
    }
  };

  const getCategoryIcon = (category: string | null) => {
    switch (category) {
      case 'Revenue':
        return <DollarSign className="w-4 h-4" />;
      case 'Customer Analytics':
        return <Users className="w-4 h-4" />;
      case 'Product Analytics':
        return <BarChart3 className="w-4 h-4" />;
      case 'Marketing':
        return <TrendingUp className="w-4 h-4" />;
      default:
        return <Database className="w-4 h-4" />;
    }
  };

  const getTierBadge = (tier: string) => {
    const colors = {
      FREE: 'bg-green-100 text-green-700 border-green-300',
      PRO: 'bg-purple-100 text-purple-700 border-purple-300',
      TEAM: 'bg-blue-100 text-blue-700 border-blue-300',
    };
    return colors[tier as keyof typeof colors] || colors.FREE;
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-gray-500">Loading recipes...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-red-50 border border-red-200 rounded-lg">
        <p className="text-red-700">Error loading recipes: {error}</p>
        <button
          onClick={loadRecipes}
          className="mt-2 px-3 py-1 bg-red-100 text-red-700 rounded hover:bg-red-200"
        >
          Retry
        </button>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="flex-shrink-0 border-b border-gray-200 bg-white px-6 py-4">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="text-xl font-semibold text-gray-900">SQL Recipe Library</h2>
            <p className="text-sm text-gray-500 mt-1">
              Pre-built analytics templates for common business questions
            </p>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-sm text-gray-600">{filteredRecipes.length} recipes</span>
          </div>
        </div>

        {/* Search Bar */}
        <div className="relative mb-4">
          <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-gray-400" />
          <input
            type="text"
            placeholder="Search recipes by name, description, or tags..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full pl-10 pr-4 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </div>

        {/* Filters */}
        <div className="flex items-center gap-4">
          {/* Tier Filter */}
          <div className="flex items-center gap-2">
            <Filter className="w-4 h-4 text-gray-500" />
            <select
              value={viewMode}
              onChange={(e) => setViewMode(e.target.value as ViewMode)}
              className="px-3 py-1.5 border border-gray-300 rounded-md text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            >
              <option value="all">All Tiers</option>
              <option value="free">FREE Only</option>
              <option value="pro">PRO/TEAM</option>
            </select>
          </div>

          {/* Data Source Filter */}
          <select
            value={dataSource}
            onChange={(e) => setDataSource(e.target.value as DataSource)}
            className="px-3 py-1.5 border border-gray-300 rounded-md text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            <option value="all">All Data Sources</option>
            <option value="Shopify">Shopify</option>
            <option value="Stripe">Stripe</option>
            <option value="Google Analytics">Google Analytics</option>
            <option value="CSV">CSV</option>
            <option value="Generic">Generic</option>
          </select>
        </div>
      </div>

      {/* Recipe Grid */}
      <div className="flex-1 overflow-y-auto p-6">
        {filteredRecipes.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-64 text-gray-500">
            <FileSpreadsheet className="w-12 h-12 mb-3 text-gray-300" />
            <p className="text-lg font-medium">No recipes found</p>
            <p className="text-sm mt-1">Try adjusting your search or filters</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {filteredRecipes.map((recipe) => (
              <div
                key={recipe.id}
                onClick={() => setSelectedRecipe(recipe)}
                className="border border-gray-200 rounded-lg p-4 hover:border-blue-400 hover:shadow-md transition-all cursor-pointer bg-white"
              >
                {/* Header */}
                <div className="flex items-start justify-between mb-3">
                  <div className="flex items-center gap-2">
                    {getCategoryIcon(recipe.category)}
                    <span className={`text-xs font-medium px-2 py-0.5 rounded border ${getTierBadge(recipe.tier)}`}>
                      {recipe.tier}
                    </span>
                  </div>
                  {recipe.metrics.rating >= 4.5 && (
                    <span title="Highly rated">
                      <Award className="w-4 h-4 text-yellow-500" />
                    </span>
                  )}
                </div>

                {/* Title */}
                <h3 className="font-semibold text-gray-900 mb-2 line-clamp-2">
                  {recipe.name}
                </h3>

                {/* Description */}
                <p className="text-sm text-gray-600 mb-3 line-clamp-3">
                  {recipe.description}
                </p>

                {/* Data Source Badge */}
                <div className="flex items-center gap-2 mb-3">
                  <Database className="w-3 h-3 text-gray-400" />
                  <span className="text-xs text-gray-500">{recipe.data_source}</span>
                </div>

                {/* Metrics */}
                <div className="flex items-center justify-between text-xs text-gray-500 border-t border-gray-100 pt-3">
                  <div className="flex items-center gap-3">
                    <span title="Times run">▶ {recipe.metrics.runs.toLocaleString()}</span>
                    <span title="Rating">★ {recipe.metrics.rating.toFixed(1)}</span>
                  </div>
                  <span className="text-gray-400">{recipe.author}</span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Recipe Detail Modal */}
      {selectedRecipe && (
        <RecipeDetailModal
          recipe={selectedRecipe}
          onClose={() => setSelectedRecipe(null)}
        />
      )}
    </div>
  );
}

interface RecipeDetailModalProps {
  recipe: Recipe;
  onClose: () => void;
}

function RecipeDetailModal({ recipe, onClose }: RecipeDetailModalProps) {
  return (
    <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50 p-4">
      <div className="bg-white rounded-lg shadow-xl max-w-3xl w-full max-h-[90vh] overflow-y-auto">
        {/* Header */}
        <div className="sticky top-0 bg-white border-b border-gray-200 px-6 py-4 flex items-start justify-between">
          <div>
            <h2 className="text-2xl font-bold text-gray-900">{recipe.name}</h2>
            <div className="flex items-center gap-3 mt-2">
              <span className={`text-xs font-medium px-2 py-1 rounded border ${recipe.tier === 'FREE' ? 'bg-green-100 text-green-700 border-green-300' : 'bg-purple-100 text-purple-700 border-purple-300'}`}>
                {recipe.tier}
              </span>
              <span className="text-sm text-gray-500">{recipe.data_source}</span>
              <span className="text-sm text-gray-400">v{recipe.version}</span>
            </div>
          </div>
          <button
            onClick={onClose}
            className="text-gray-400 hover:text-gray-600 text-2xl font-bold"
          >
            ×
          </button>
        </div>

        {/* Content */}
        <div className="px-6 py-4 space-y-6">
          {/* Description */}
          <div>
            <h3 className="font-semibold text-gray-900 mb-2">Description</h3>
            <p className="text-gray-700">{recipe.description}</p>
          </div>

          {/* Metrics */}
          <div className="grid grid-cols-4 gap-4">
            <div className="text-center p-3 bg-gray-50 rounded-lg">
              <div className="text-2xl font-bold text-gray-900">{recipe.metrics.runs.toLocaleString()}</div>
              <div className="text-xs text-gray-500 mt-1">Times Run</div>
            </div>
            <div className="text-center p-3 bg-gray-50 rounded-lg">
              <div className="text-2xl font-bold text-gray-900">★ {recipe.metrics.rating.toFixed(1)}</div>
              <div className="text-xs text-gray-500 mt-1">Rating</div>
            </div>
            <div className="text-center p-3 bg-gray-50 rounded-lg">
              <div className="text-2xl font-bold text-gray-900">{recipe.metrics.downloads.toLocaleString()}</div>
              <div className="text-xs text-gray-500 mt-1">Downloads</div>
            </div>
            <div className="text-center p-3 bg-gray-50 rounded-lg">
              <div className="text-2xl font-bold text-gray-900">{recipe.metrics.review_count}</div>
              <div className="text-xs text-gray-500 mt-1">Reviews</div>
            </div>
          </div>

          {/* Required Tables */}
          <div>
            <h3 className="font-semibold text-gray-900 mb-2">Required Tables</h3>
            <div className="space-y-2">
              {recipe.required_tables.map((table, idx) => (
                <div key={idx} className="p-3 bg-blue-50 border border-blue-200 rounded-lg">
                  <div className="font-medium text-blue-900">{table.name}</div>
                  {table.required_columns.length > 0 && (
                    <div className="text-sm text-blue-700 mt-1">
                      Columns: {table.required_columns.join(', ')}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>

          {/* Parameters */}
          {recipe.parameters.length > 0 && (
            <div>
              <h3 className="font-semibold text-gray-900 mb-2">Parameters</h3>
              <div className="space-y-2">
                {recipe.parameters.map((param, idx) => (
                  <div key={idx} className="p-3 bg-gray-50 border border-gray-200 rounded-lg">
                    <div className="flex items-center justify-between">
                      <span className="font-medium text-gray-900">{param.label || param.name}</span>
                      <span className="text-xs text-gray-500 uppercase">{param.type}</span>
                    </div>
                    {param.description && (
                      <p className="text-sm text-gray-600 mt-1">{param.description}</p>
                    )}
                    {param.default !== null && param.default !== undefined && (
                      <div className="text-xs text-gray-500 mt-1">
                        Default: {String(param.default)}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Tags */}
          {recipe.tags.length > 0 && (
            <div>
              <h3 className="font-semibold text-gray-900 mb-2">Tags</h3>
              <div className="flex flex-wrap gap-2">
                {recipe.tags.map((tag, idx) => (
                  <span
                    key={idx}
                    className="px-2 py-1 bg-gray-100 text-gray-700 text-xs rounded-full"
                  >
                    {tag}
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="sticky bottom-0 bg-gray-50 border-t border-gray-200 px-6 py-4 flex items-center justify-between">
          <div className="text-sm text-gray-500">
            By {recipe.author}
          </div>
          <div className="flex gap-3">
            <button
              onClick={onClose}
              className="px-4 py-2 text-gray-700 hover:bg-gray-200 rounded-lg"
            >
              Close
            </button>
            <button
              onClick={() => {
                // TODO: Navigate to recipe execution page
                console.log('Execute recipe:', recipe.id);
              }}
              className="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium"
            >
              Run Recipe →
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
