// Folder detail modal — shows datasets, schemas, and compatible recipes
// Helps users understand what data they have before analyzing

import { useState, useEffect } from 'react';
import {
  X,
  Database,
  Table,
  FileText,
  Sparkles,
  ChevronRight,
  Loader2,
} from 'lucide-react';
import type { WatchedFolder } from '../types/events';

interface Dataset {
  name: string;
  path: string;
  row_count: number | null;
  column_count: number;
  file_size: number;
  columns: Array<{
    name: string;
    type: string;
  }>;
}

interface FolderDetailModalProps {
  folder: WatchedFolder;
  onClose: () => void;
  onAnalyze?: (dataSource?: string) => void;
}

export function FolderDetailModal({ folder, onClose, onAnalyze }: FolderDetailModalProps) {
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expandedDataset, setExpandedDataset] = useState<string | null>(null);

  useEffect(() => {
    const loadDatasets = async () => {
      try {
        setLoading(true);
        setError(null);

        // TODO: Add backend command to fetch dataset details for a folder
        // For now, simulate with mock data
        await new Promise(resolve => setTimeout(resolve, 500));

        // Mock data - replace with actual invoke call
        const mockDatasets: Dataset[] = [
          {
            name: 'orders.csv',
            path: `${folder.path}/orders.csv`,
            row_count: 15420,
            column_count: 8,
            file_size: 2_500_000,
            columns: [
              { name: 'order_id', type: 'INTEGER' },
              { name: 'customer_id', type: 'INTEGER' },
              { name: 'order_date', type: 'TIMESTAMP' },
              { name: 'total_amount', type: 'DECIMAL' },
              { name: 'status', type: 'VARCHAR' },
              { name: 'product_id', type: 'INTEGER' },
              { name: 'quantity', type: 'INTEGER' },
              { name: 'discount', type: 'DECIMAL' },
            ],
          },
          {
            name: 'customers.csv',
            path: `${folder.path}/customers.csv`,
            row_count: 3240,
            column_count: 6,
            file_size: 890_000,
            columns: [
              { name: 'customer_id', type: 'INTEGER' },
              { name: 'email', type: 'VARCHAR' },
              { name: 'name', type: 'VARCHAR' },
              { name: 'created_at', type: 'TIMESTAMP' },
              { name: 'country', type: 'VARCHAR' },
              { name: 'total_spent', type: 'DECIMAL' },
            ],
          },
          {
            name: 'products.csv',
            path: `${folder.path}/products.csv`,
            row_count: 856,
            column_count: 5,
            file_size: 125_000,
            columns: [
              { name: 'product_id', type: 'INTEGER' },
              { name: 'name', type: 'VARCHAR' },
              { name: 'price', type: 'DECIMAL' },
              { name: 'category', type: 'VARCHAR' },
              { name: 'stock', type: 'INTEGER' },
            ],
          },
        ];

        setDatasets(mockDatasets);
      } catch (err) {
        console.error('Failed to load datasets:', err);
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setLoading(false);
      }
    };

    loadDatasets();
  }, [folder.path]);

  // Detect data source from folder path
  const detectDataSource = (): string => {
    const pathLower = folder.path.toLowerCase();
    if (pathLower.includes('shopify')) return 'Shopify';
    if (pathLower.includes('stripe')) return 'Stripe';
    if (pathLower.includes('analytics') || pathLower.includes('ga')) return 'Google Analytics';
    return 'Generic';
  };

  const dataSource = detectDataSource();

  // Suggest compatible recipes based on detected columns
  const suggestRecipes = (): string[] => {
    const allColumns = datasets.flatMap(d => d.columns.map(c => c.name.toLowerCase()));
    const suggestions: string[] = [];

    if (allColumns.includes('order_id') || allColumns.includes('order_date')) {
      suggestions.push('Revenue Trends', 'Top Products', 'Sales by Period');
    }
    if (allColumns.includes('customer_id') || allColumns.includes('email')) {
      suggestions.push('Customer Cohorts', 'Retention Analysis');
    }
    if (allColumns.includes('product_id') || allColumns.includes('product_name')) {
      suggestions.push('Product Performance', 'Inventory Analysis');
    }

    return suggestions.length > 0 ? suggestions : ['Generic SQL Queries'];
  };

  const suggestedRecipes = suggestRecipes();

  const formatBytes = (bytes: number): string => {
    if (bytes === 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div className="relative flex max-h-[90vh] w-full max-w-4xl flex-col overflow-hidden rounded-xl bg-white shadow-2xl dark:bg-slate-900">
        {/* Header */}
        <div className="flex items-start justify-between border-b border-slate-200 p-6 dark:border-slate-800">
          <div className="min-w-0 flex-1">
            <h2 className="text-xl font-bold text-slate-900 dark:text-slate-50">
              Folder Details
            </h2>
            <p className="mt-1 truncate text-sm text-slate-600 dark:text-slate-400" title={folder.path}>
              {folder.path}
            </p>
          </div>
          <button
            onClick={onClose}
            className="ml-4 rounded-lg p-2 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-6">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="h-8 w-8 animate-spin text-purple-600 dark:text-purple-400" />
            </div>
          ) : error ? (
            <div className="rounded-lg border border-red-200 bg-red-50 p-4 text-sm text-red-700 dark:border-red-900 dark:bg-red-950 dark:text-red-300">
              Error loading datasets: {error}
            </div>
          ) : (
            <div className="space-y-6">
              {/* Summary Stats */}
              <div className="grid grid-cols-3 gap-4">
                <div className="rounded-lg border border-slate-200 bg-slate-50 p-4 dark:border-slate-700 dark:bg-slate-800/50">
                  <div className="flex items-center gap-2 text-sm font-medium text-slate-600 dark:text-slate-400">
                    <Database className="h-4 w-4" />
                    Datasets
                  </div>
                  <div className="mt-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
                    {datasets.length}
                  </div>
                </div>
                <div className="rounded-lg border border-slate-200 bg-slate-50 p-4 dark:border-slate-700 dark:bg-slate-800/50">
                  <div className="flex items-center gap-2 text-sm font-medium text-slate-600 dark:text-slate-400">
                    <Table className="h-4 w-4" />
                    Total Rows
                  </div>
                  <div className="mt-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
                    {datasets.reduce((sum, d) => sum + (d.row_count || 0), 0).toLocaleString()}
                  </div>
                </div>
                <div className="rounded-lg border border-slate-200 bg-slate-50 p-4 dark:border-slate-700 dark:bg-slate-800/50">
                  <div className="flex items-center gap-2 text-sm font-medium text-slate-600 dark:text-slate-400">
                    <FileText className="h-4 w-4" />
                    Total Size
                  </div>
                  <div className="mt-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
                    {formatBytes(datasets.reduce((sum, d) => sum + d.file_size, 0))}
                  </div>
                </div>
              </div>

              {/* Compatible Recipes */}
              {onAnalyze && suggestedRecipes.length > 0 && (
                <div className="rounded-lg border border-purple-200 bg-purple-50 p-4 dark:border-purple-800 dark:bg-purple-950/40">
                  <div className="mb-3 flex items-center justify-between">
                    <div className="flex items-center gap-2 font-semibold text-purple-900 dark:text-purple-200">
                      <Sparkles className="h-4 w-4" />
                      Compatible Recipes
                    </div>
                    <span className="text-xs text-purple-700 dark:text-purple-300">
                      Detected: {dataSource}
                    </span>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {suggestedRecipes.map((recipe, idx) => (
                      <span
                        key={idx}
                        className="rounded-md border border-purple-300 bg-white px-3 py-1 text-sm text-purple-800 dark:border-purple-700 dark:bg-purple-900/50 dark:text-purple-200"
                      >
                        {recipe}
                      </span>
                    ))}
                  </div>
                  <button
                    onClick={() => {
                      onAnalyze(dataSource);
                      onClose();
                    }}
                    className="mt-3 flex w-full items-center justify-center gap-2 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-purple-700"
                  >
                    <Sparkles className="h-4 w-4" />
                    Analyze with These Recipes
                  </button>
                </div>
              )}

              {/* Dataset List */}
              <div>
                <h3 className="mb-3 text-sm font-semibold uppercase tracking-wide text-slate-600 dark:text-slate-400">
                  Datasets ({datasets.length})
                </h3>
                <div className="space-y-2">
                  {datasets.map((dataset) => {
                    const isExpanded = expandedDataset === dataset.name;
                    return (
                      <div
                        key={dataset.name}
                        className="overflow-hidden rounded-lg border border-slate-200 bg-white dark:border-slate-700 dark:bg-slate-800"
                      >
                        {/* Dataset header */}
                        <button
                          onClick={() => setExpandedDataset(isExpanded ? null : dataset.name)}
                          className="flex w-full items-center justify-between p-4 text-left transition-colors hover:bg-slate-50 dark:hover:bg-slate-700/50"
                        >
                          <div className="flex min-w-0 items-center gap-3">
                            <Database className="h-5 w-5 shrink-0 text-slate-600 dark:text-slate-400" />
                            <div className="min-w-0 flex-1">
                              <div className="truncate font-medium text-slate-900 dark:text-slate-50">
                                {dataset.name}
                              </div>
                              <div className="mt-0.5 text-xs text-slate-500 dark:text-slate-400">
                                {dataset.row_count?.toLocaleString() || '?'} rows · {dataset.column_count} columns · {formatBytes(dataset.file_size)}
                              </div>
                            </div>
                          </div>
                          <ChevronRight
                            className={`h-5 w-5 shrink-0 text-slate-400 transition-transform ${
                              isExpanded ? 'rotate-90' : ''
                            }`}
                          />
                        </button>

                        {/* Dataset schema (expandable) */}
                        {isExpanded && (
                          <div className="border-t border-slate-200 bg-slate-50 p-4 dark:border-slate-700 dark:bg-slate-900/50">
                            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-600 dark:text-slate-400">
                              Schema
                            </div>
                            <div className="space-y-1">
                              {dataset.columns.map((col, idx) => (
                                <div
                                  key={idx}
                                  className="flex items-center justify-between rounded border border-slate-200 bg-white px-3 py-1.5 text-sm dark:border-slate-700 dark:bg-slate-800"
                                >
                                  <span className="font-medium text-slate-900 dark:text-slate-50">
                                    {col.name}
                                  </span>
                                  <span className="rounded bg-slate-100 px-2 py-0.5 text-xs font-mono text-slate-600 dark:bg-slate-700 dark:text-slate-300">
                                    {col.type}
                                  </span>
                                </div>
                              ))}
                            </div>
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="border-t border-slate-200 p-4 dark:border-slate-800">
          <div className="flex items-center justify-between text-xs text-slate-500 dark:text-slate-400">
            <span>
              Click on a dataset to view its schema
            </span>
            <button
              onClick={onClose}
              className="rounded-lg border border-slate-300 px-4 py-2 font-medium text-slate-700 transition-colors hover:bg-slate-100 dark:border-slate-600 dark:text-slate-300 dark:hover:bg-slate-800"
            >
              Close
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
