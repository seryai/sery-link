import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Play, AlertCircle, CheckCircle2, Download, X } from 'lucide-react';
import { UpgradePrompt } from './UpgradePrompt';

interface Recipe {
  id: string;
  name: string;
  description: string;
  data_source: string;
  tier: 'FREE' | 'PRO' | 'TEAM';
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
    validation?: {
      min?: any;
      max?: any;
      pattern?: string;
      options?: string[];
    };
  }>;
}

interface RecipeExecutorProps {
  recipe: Recipe;
  onClose: () => void;
}

interface QueryResult {
  columns: string[];
  rows: Record<string, any>[];
  row_count: number;
  execution_time_ms: number;
}

export function RecipeExecutor({ recipe, onClose }: RecipeExecutorProps) {
  const [parameters, setParameters] = useState<Record<string, any>>(() => {
    const defaults: Record<string, any> = {};
    recipe.parameters.forEach(param => {
      if (param.default !== null && param.default !== undefined) {
        defaults[param.name] = param.default;
      }
    });
    return defaults;
  });

  const [executing, setExecuting] = useState(false);
  const [result, setResult] = useState<QueryResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [renderedSql, setRenderedSql] = useState<string | null>(null);
  const [showUpgradePrompt, setShowUpgradePrompt] = useState(false);

  const handleParameterChange = (paramName: string, value: any) => {
    setParameters(prev => ({
      ...prev,
      [paramName]: value
    }));
  };


  const executeRecipe = async () => {
    try {
      setExecuting(true);
      setError(null);
      setResult(null);

      // Step 1: Execute recipe (with tier check)
      const sql = await invoke<string>('execute_recipe', {
        recipeId: recipe.id,
        params: parameters
      });

      setRenderedSql(sql);

      // Step 2: Execute SQL via DuckDB
      // TODO: This requires implementing a execute_sql Tauri command
      // For now, we'll simulate the execution
      const mockResult: QueryResult = {
        columns: ['column1', 'column2', 'column3'],
        rows: [
          { column1: 'value1', column2: 'value2', column3: 'value3' },
          { column1: 'value4', column2: 'value5', column3: 'value6' },
        ],
        row_count: 2,
        execution_time_ms: 123
      };

      setResult(mockResult);
    } catch (err) {
      console.error('Recipe execution failed:', err);
      const errorMsg = err instanceof Error ? err.message : String(err);

      // Check if it's a tier restriction error
      if (errorMsg.includes('requires PRO tier') || errorMsg.includes('requires TEAM tier')) {
        setShowUpgradePrompt(true);
      } else {
        setError(errorMsg);
      }
    } finally {
      setExecuting(false);
    }
  };

  const downloadCsv = () => {
    if (!result) return;

    // Convert result to CSV
    const csvRows = [
      result.columns.join(','), // Header
      ...result.rows.map(row =>
        result.columns.map(col => {
          const value = row[col];
          // Escape quotes and wrap in quotes if contains comma
          const escaped = String(value).replace(/"/g, '""');
          return escaped.includes(',') ? `"${escaped}"` : escaped;
        }).join(',')
      )
    ];

    const csv = csvRows.join('\n');
    const blob = new Blob([csv], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${recipe.id.replace(/\./g, '-')}-${Date.now()}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50 p-4">
      <div className="bg-white rounded-lg shadow-xl max-w-5xl w-full max-h-[90vh] overflow-y-auto">
        {/* Header */}
        <div className="sticky top-0 bg-white border-b border-gray-200 px-6 py-4 flex items-start justify-between z-10">
          <div>
            <h2 className="text-2xl font-bold text-gray-900">{recipe.name}</h2>
            <p className="text-sm text-gray-500 mt-1">Configure parameters and run</p>
          </div>
          <button
            onClick={onClose}
            className="text-gray-400 hover:text-gray-600 text-2xl font-bold"
          >
            <X className="w-6 h-6" />
          </button>
        </div>

        <div className="px-6 py-4 space-y-6">
          {/* Parameter Form */}
          {recipe.parameters.length > 0 ? (
            <div>
              <h3 className="font-semibold text-gray-900 mb-4">Parameters</h3>
              <div className="space-y-4">
                {recipe.parameters.map(param => (
                  <div key={param.name} className="space-y-2">
                    <label className="block text-sm font-medium text-gray-700">
                      {param.label || param.name}
                      {param.description && (
                        <span className="ml-2 text-gray-500 font-normal">
                          — {param.description}
                        </span>
                      )}
                    </label>

                    {/* Input based on parameter type */}
                    {param.type === 'date' && (
                      <input
                        type="date"
                        value={parameters[param.name] || ''}
                        onChange={(e) => handleParameterChange(param.name, e.target.value)}
                        className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    )}

                    {param.type === 'int' && (
                      <input
                        type="number"
                        step="1"
                        value={parameters[param.name] || ''}
                        onChange={(e) => handleParameterChange(param.name, parseInt(e.target.value) || 0)}
                        min={param.validation?.min}
                        max={param.validation?.max}
                        className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    )}

                    {param.type === 'float' && (
                      <input
                        type="number"
                        step="0.01"
                        value={parameters[param.name] || ''}
                        onChange={(e) => handleParameterChange(param.name, parseFloat(e.target.value) || 0)}
                        min={param.validation?.min}
                        max={param.validation?.max}
                        className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    )}

                    {param.type === 'string' && param.validation?.options ? (
                      <select
                        value={parameters[param.name] || ''}
                        onChange={(e) => handleParameterChange(param.name, e.target.value)}
                        className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                      >
                        <option value="">Select...</option>
                        {param.validation.options.map(opt => (
                          <option key={opt} value={opt}>{opt}</option>
                        ))}
                      </select>
                    ) : param.type === 'string' ? (
                      <input
                        type="text"
                        value={parameters[param.name] || ''}
                        onChange={(e) => handleParameterChange(param.name, e.target.value)}
                        pattern={param.validation?.pattern}
                        className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    ) : null}

                    {param.type === 'boolean' && (
                      <label className="flex items-center gap-2">
                        <input
                          type="checkbox"
                          checked={parameters[param.name] || false}
                          onChange={(e) => handleParameterChange(param.name, e.target.checked)}
                          className="w-4 h-4 text-blue-600 border-gray-300 rounded focus:ring-2 focus:ring-blue-500"
                        />
                        <span className="text-sm text-gray-600">Enable</span>
                      </label>
                    )}

                    {/* Validation hints */}
                    {param.validation && (
                      <div className="text-xs text-gray-500">
                        {param.validation.min !== undefined && param.validation.max !== undefined && (
                          <span>Range: {param.validation.min} - {param.validation.max}</span>
                        )}
                        {param.validation.pattern && (
                          <span>Pattern: {param.validation.pattern}</span>
                        )}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            </div>
          ) : (
            <div className="text-sm text-gray-500 italic">
              This recipe has no configurable parameters.
            </div>
          )}

          {/* Rendered SQL Preview */}
          {renderedSql && (
            <div>
              <h3 className="font-semibold text-gray-900 mb-2">Generated SQL</h3>
              <pre className="bg-gray-900 text-gray-100 p-4 rounded-lg overflow-x-auto text-sm">
                {renderedSql}
              </pre>
            </div>
          )}

          {/* Error Display */}
          {error && (
            <div className="p-4 bg-red-50 border border-red-200 rounded-lg flex items-start gap-3">
              <AlertCircle className="w-5 h-5 text-red-600 flex-shrink-0 mt-0.5" />
              <div>
                <div className="font-medium text-red-900">Execution Failed</div>
                <div className="text-sm text-red-700 mt-1">{error}</div>
              </div>
            </div>
          )}

          {/* Results Display */}
          {result && (
            <div>
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2">
                  <CheckCircle2 className="w-5 h-5 text-green-600" />
                  <h3 className="font-semibold text-gray-900">Results</h3>
                  <span className="text-sm text-gray-500">
                    ({result.row_count} rows, {result.execution_time_ms}ms)
                  </span>
                </div>
                <button
                  onClick={downloadCsv}
                  className="flex items-center gap-2 px-3 py-1.5 text-sm bg-gray-100 hover:bg-gray-200 text-gray-700 rounded-lg"
                >
                  <Download className="w-4 h-4" />
                  Download CSV
                </button>
              </div>

              <div className="border border-gray-200 rounded-lg overflow-hidden">
                <div className="overflow-x-auto max-h-96">
                  <table className="w-full text-sm">
                    <thead className="bg-gray-50 border-b border-gray-200 sticky top-0">
                      <tr>
                        {result.columns.map(col => (
                          <th
                            key={col}
                            className="px-4 py-2 text-left font-medium text-gray-700"
                          >
                            {col}
                          </th>
                        ))}
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-gray-200">
                      {result.rows.map((row, idx) => (
                        <tr key={idx} className="hover:bg-gray-50">
                          {result.columns.map(col => (
                            <td key={col} className="px-4 py-2 text-gray-900">
                              {String(row[col])}
                            </td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="sticky bottom-0 bg-gray-50 border-t border-gray-200 px-6 py-4 flex items-center justify-between">
          <div className="text-sm text-gray-500">
            {recipe.tier === 'PRO' || recipe.tier === 'TEAM' ? (
              <span className="text-purple-600 font-medium">PRO Recipe</span>
            ) : (
              <span className="text-green-600 font-medium">FREE Recipe</span>
            )}
          </div>
          <div className="flex gap-3">
            <button
              onClick={onClose}
              disabled={executing}
              className="px-4 py-2 text-gray-700 hover:bg-gray-200 rounded-lg disabled:opacity-50"
            >
              Close
            </button>
            <button
              onClick={executeRecipe}
              disabled={executing}
              className="flex items-center gap-2 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {executing ? (
                <>
                  <div className="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin" />
                  Executing...
                </>
              ) : (
                <>
                  <Play className="w-4 h-4" />
                  Run Recipe
                </>
              )}
            </button>
          </div>
        </div>
      </div>

      {/* Upgrade Modal */}
      {showUpgradePrompt && (
        <UpgradePrompt
          variant="modal"
          feature="recipe"
          onClose={() => {
            setShowUpgradePrompt(false);
            onClose();
          }}
          onUpgrade={() => {
            setShowUpgradePrompt(false);
            onClose();
          }}
        />
      )}
    </div>
  );
}
