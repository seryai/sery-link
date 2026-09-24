// Products — what this machine sells.
//
// The publish flow under DECISIONS.md 2026-09-23: pick a file, see what a
// buyer would get (columns, size, PII-looking columns), give it a table
// name and a price, and push it. Everything after "Publish" happens on
// the Rust side (publish.rs): convert to Parquet in a temp dir, stream to
// the presigned URL, confirm with the api. Buyers are served from the
// cloud copy; this machine can go to sleep.

import { useCallback, useEffect, useMemo, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  AlertTriangle,
  Check,
  Copy,
  Database,
  FileText,
  Loader2,
  Plus,
  Store,
  Trash2,
  Upload,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import { isRemoteUrl } from '../utils/url';
import type { DatasetMetadataPayload, PublishProgressPayload } from '../types/events';

// ── types (mirror api/app/api/v1/products.py) ─────────────────────────

type ProductKind = 'tabular' | 'document';

interface Product {
  id: string;
  hash: string;
  name: string;
  description: string | null;
  tags: string[];
  kind: ProductKind;
  price_per_call: number;
  listed: boolean;
  password_required: boolean;
  dataset_count: number;
  storage_bytes: number;
  query_count: number;
  published_at: string | null;
  created_at: string;
}

interface ProductDataset {
  dataset_id: string;
  table: string | null;
  query_path: string;
  file_format: string;
  size_bytes: number;
  row_count_estimate: number | null;
  snapshot_at: string | null;
}

interface Precheck {
  relative_path: string;
  file_format: string;
  kind: ProductKind | 'unsupported';
  size_bytes: number;
  row_count_estimate: number | null;
  columns: { name: string; type: string; nullable: boolean }[];
  pii_columns: string[];
  suggested_table: string;
}

interface Published {
  dataset_id: string;
  table: string | null;
  size_bytes: number;
  passages: number;
}

const TABULAR = new Set(['csv', 'tsv', 'xlsx', 'xls', 'parquet']);
const DOCUMENT = new Set(['pdf', 'docx', 'pptx', 'html', 'htm', 'ipynb', 'epub', 'rtf', 'md', 'txt']);

const inputCls =
  'w-full rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm text-slate-800 focus:border-purple-500 focus:outline-none focus:ring-1 focus:ring-purple-500 dark:border-slate-600 dark:bg-slate-800 dark:text-slate-100';
const primaryCls =
  'inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60';
const secondaryCls =
  'inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800';

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

function errText(err: unknown): string {
  return typeof err === 'string' ? err : err instanceof Error ? err.message : String(err);
}

// ── page ──────────────────────────────────────────────────────────────

export function Products() {
  const toast = useToast();
  const [params, setParams] = useSearchParams();
  const [products, setProducts] = useState<Product[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  // Deep link from FileDetail: /products?folder=…&file=…
  const preselect = useMemo(() => {
    const folder = params.get('folder');
    const file = params.get('file');
    return folder && file ? { folder, file } : null;
  }, [params]);

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<Product[]>('publish_list_products');
      setProducts(list);
      setError(null);
      return list;
    } catch (err) {
      setError(errText(err));
      return [];
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh().then((list) => {
      if (preselect) {
        const ext = preselect.file.split('.').pop()?.toLowerCase() ?? '';
        const kind: ProductKind = DOCUMENT.has(ext) ? 'document' : 'tabular';
        const match = list.find((p) => p.kind === kind);
        if (match) setSelected(match.hash);
        else setCreating(true);
      } else if (list.length > 0 && !selected) {
        setSelected(list[0].hash);
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refresh]);

  const current = products.find((p) => p.hash === selected) ?? null;

  return (
    <div className="p-6">
      <div className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="flex items-center gap-2 text-xl font-semibold text-slate-800 dark:text-slate-100">
            <Store className="h-5 w-5 text-purple-600 dark:text-purple-400" />
            Products
          </h1>
          <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
            Datasets you sell. Buyers query a copy Sery hosts; this machine only publishes.
          </p>
        </div>
        <button className={primaryCls} onClick={() => setCreating(true)}>
          <Plus className="h-4 w-4" />
          New product
        </button>
      </div>

      {error && (
        <div className="mb-4 rounded-md border border-amber-200 bg-amber-50 p-3 text-sm text-amber-800 dark:border-amber-900/50 dark:bg-amber-900/20 dark:text-amber-200">
          {error.includes('Not signed in')
            ? 'Connect this machine to a workspace before publishing.'
            : error}
        </div>
      )}

      {creating && (
        <CreateProduct
          defaultKind={preselect ? (DOCUMENT.has(preselect.file.split('.').pop()?.toLowerCase() ?? '') ? 'document' : 'tabular') : 'tabular'}
          onCancel={() => setCreating(false)}
          onCreated={async (p) => {
            setCreating(false);
            await refresh();
            setSelected(p.hash);
            toast.success(`Created ${p.name}`);
          }}
        />
      )}

      {loading ? (
        <div className="flex items-center gap-2 text-sm text-slate-500">
          <Loader2 className="h-4 w-4 animate-spin" /> Loading…
        </div>
      ) : products.length === 0 && !creating ? (
        <div className="rounded-xl border border-dashed border-slate-300 p-10 text-center text-sm text-slate-500 dark:border-slate-700 dark:text-slate-400">
          Nothing for sale yet. Create a product, then add a CSV, Parquet, Excel file or a document corpus.
        </div>
      ) : (
        <div className="grid gap-6 lg:grid-cols-[280px_1fr]">
          <ul className="space-y-1">
            {products.map((p) => (
              <li key={p.hash}>
                <button
                  onClick={() => setSelected(p.hash)}
                  className={`w-full rounded-lg px-3 py-2 text-left text-sm ${
                    p.hash === selected
                      ? 'bg-purple-50 text-purple-900 dark:bg-purple-900/30 dark:text-purple-100'
                      : 'text-slate-700 hover:bg-slate-50 dark:text-slate-200 dark:hover:bg-slate-800'
                  }`}
                >
                  <div className="flex items-center gap-2">
                    {p.kind === 'document' ? (
                      <FileText className="h-4 w-4 shrink-0 text-slate-400" />
                    ) : (
                      <Database className="h-4 w-4 shrink-0 text-slate-400" />
                    )}
                    <span className="truncate font-medium">{p.name}</span>
                  </div>
                  <div className="mt-0.5 flex items-center gap-2 text-xs text-slate-500 dark:text-slate-400">
                    <span>{p.dataset_count} dataset{p.dataset_count === 1 ? '' : 's'}</span>
                    <span>·</span>
                    <span>{p.price_per_call} tok/call</span>
                    {p.listed ? (
                      <span className="rounded bg-emerald-100 px-1.5 text-[10px] font-semibold text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300">
                        LISTED
                      </span>
                    ) : (
                      <span className="rounded bg-slate-100 px-1.5 text-[10px] font-semibold text-slate-500 dark:bg-slate-800">
                        DRAFT
                      </span>
                    )}
                  </div>
                </button>
              </li>
            ))}
          </ul>

          {current && (
            <ProductPanel
              key={current.hash}
              product={current}
              preselect={preselect && current.kind === (DOCUMENT.has(preselect.file.split('.').pop()?.toLowerCase() ?? '') ? 'document' : 'tabular') ? preselect : null}
              onChanged={async () => {
                await refresh();
                if (preselect) setParams({});
              }}
              onWithdrawn={async () => {
                setSelected(null);
                await refresh();
              }}
            />
          )}
        </div>
      )}
    </div>
  );
}

// ── create ────────────────────────────────────────────────────────────

function CreateProduct({
  defaultKind,
  onCancel,
  onCreated,
}: {
  defaultKind: ProductKind;
  onCancel: () => void;
  onCreated: (p: Product) => void;
}) {
  const toast = useToast();
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [kind, setKind] = useState<ProductKind>(defaultKind);
  const [price, setPrice] = useState(5);
  const [tags, setTags] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!name.trim()) return;
    setBusy(true);
    try {
      const p = await invoke<Product>('publish_create_product', {
        product: {
          name: name.trim(),
          description: description.trim() || null,
          tags: tags.split(',').map((t) => t.trim()).filter(Boolean),
          kind,
          price_per_call: price,
          password: password || null,
        },
      });
      onCreated(p);
    } catch (err) {
      toast.error(`Could not create product: ${errText(err)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mb-6 rounded-xl border border-slate-200 bg-white p-4 dark:border-slate-700 dark:bg-slate-900">
      <h2 className="mb-3 text-sm font-semibold text-slate-800 dark:text-slate-100">New product</h2>
      <div className="grid gap-3 md:grid-cols-2">
        <label className="text-xs text-slate-600 dark:text-slate-300">
          Name
          <input className={`mt-1 ${inputCls}`} value={name} onChange={(e) => setName(e.target.value)} placeholder="Vancouver school catchments" />
        </label>
        <label className="text-xs text-slate-600 dark:text-slate-300">
          Price per call (tokens)
          <input type="number" min={1} className={`mt-1 ${inputCls}`} value={price} onChange={(e) => setPrice(Math.max(1, Number(e.target.value) || 1))} />
        </label>
        <label className="text-xs text-slate-600 dark:text-slate-300 md:col-span-2">
          Description
          <textarea className={`mt-1 ${inputCls}`} rows={2} value={description} onChange={(e) => setDescription(e.target.value)} placeholder="What is in it, where it came from, how often it updates." />
        </label>
        <label className="text-xs text-slate-600 dark:text-slate-300">
          Tags (comma separated)
          <input className={`mt-1 ${inputCls}`} value={tags} onChange={(e) => setTags(e.target.value)} placeholder="real-estate, canada" />
        </label>
        <label className="text-xs text-slate-600 dark:text-slate-300">
          Password (optional)
          <input type="password" className={`mt-1 ${inputCls}`} value={password} onChange={(e) => setPassword(e.target.value)} placeholder="Leave empty for a public product" />
        </label>
        <div className="text-xs text-slate-600 dark:text-slate-300 md:col-span-2">
          Kind
          <div className="mt-1 flex gap-4">
            <label className="flex items-center gap-1.5">
              <input type="radio" checked={kind === 'tabular'} onChange={() => setKind('tabular')} />
              Tabular — CSV, Parquet, Excel; buyers run SQL
            </label>
            <label className="flex items-center gap-1.5">
              <input type="radio" checked={kind === 'document'} onChange={() => setKind('document')} />
              Documents — PDF, DOCX…; buyers search and get cited passages
            </label>
          </div>
        </div>
      </div>
      <div className="mt-4 flex justify-end gap-2">
        <button className={secondaryCls} onClick={onCancel}>Cancel</button>
        <button className={primaryCls} disabled={busy || !name.trim()} onClick={submit}>
          {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
          Create
        </button>
      </div>
    </div>
  );
}

// ── product panel ─────────────────────────────────────────────────────

function ProductPanel({
  product,
  preselect,
  onChanged,
  onWithdrawn,
}: {
  product: Product;
  preselect: { folder: string; file: string } | null;
  onChanged: () => Promise<void>;
  onWithdrawn: () => Promise<void>;
}) {
  const toast = useToast();
  const { config } = useAgentStore();
  const [datasets, setDatasets] = useState<ProductDataset[]>([]);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  const loadDatasets = useCallback(async () => {
    try {
      setDatasets(await invoke<ProductDataset[]>('publish_list_datasets', { hash: product.hash }));
    } catch (err) {
      toast.error(errText(err));
    }
  }, [product.hash, toast]);

  useEffect(() => {
    void loadDatasets();
  }, [loadDatasets]);

  const patch = async (p: Record<string, unknown>) => {
    setBusy(true);
    try {
      await invoke('publish_update_product', { hash: product.hash, patch: p });
      await onChanged();
    } catch (err) {
      toast.error(errText(err));
    } finally {
      setBusy(false);
    }
  };

  const withdraw = async () => {
    setBusy(true);
    try {
      await invoke('publish_withdraw_product', { hash: product.hash });
      toast.success(`Withdrew ${product.name}. Your files are untouched.`);
      await onWithdrawn();
    } catch (err) {
      toast.error(errText(err));
    } finally {
      setBusy(false);
    }
  };

  const removeDataset = async (d: ProductDataset) => {
    try {
      await invoke('publish_remove_dataset', { hash: product.hash, datasetId: d.dataset_id });
      await loadDatasets();
      await onChanged();
    } catch (err) {
      toast.error(errText(err));
    }
  };

  const localFolders = (config?.watched_folders ?? []).map((f) => f.path).filter((p) => !isRemoteUrl(p));
  const apiBase = (config?.cloud.api_url ?? '').replace(/\/$/, '');

  return (
    <div className="space-y-6">
      <div className="rounded-xl border border-slate-200 bg-white p-4 dark:border-slate-700 dark:bg-slate-900">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h2 className="truncate text-lg font-semibold text-slate-900 dark:text-slate-50">{product.name}</h2>
            {product.description && (
              <p className="mt-1 text-sm text-slate-600 dark:text-slate-300">{product.description}</p>
            )}
            <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-slate-500 dark:text-slate-400">
              <code className="rounded bg-slate-100 px-1.5 py-0.5 dark:bg-slate-800">{product.hash}</code>
              <button
                className="inline-flex items-center gap-1 hover:text-slate-800 dark:hover:text-slate-100"
                onClick={async () => {
                  await navigator.clipboard.writeText(`${apiBase}/v1/products/${product.hash}`);
                  setCopied(true);
                  setTimeout(() => setCopied(false), 1500);
                }}
                title="Copy the product URL buyers and agents use"
              >
                {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
                {copied ? 'Copied' : 'Copy URL'}
              </button>
              <span>·</span>
              <span>{product.price_per_call} tokens per call</span>
              <span>·</span>
              <span>{fmtBytes(product.storage_bytes)} hosted</span>
              <span>·</span>
              <span>{product.query_count} paid calls</span>
              {product.password_required && (<><span>·</span><span>password protected</span></>)}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <button
              className={product.listed ? secondaryCls : primaryCls}
              disabled={busy || (!product.listed && !product.published_at)}
              title={!product.published_at ? 'Publish at least one dataset before listing' : undefined}
              onClick={() => patch({ listed: !product.listed })}
            >
              {product.listed ? 'Unlist' : 'List on marketplace'}
            </button>
            <button className={secondaryCls} disabled={busy} onClick={withdraw} title="Remove the product and delete Sery's copies. Your files stay where they are.">
              <Trash2 className="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>

      <div className="rounded-xl border border-slate-200 bg-white dark:border-slate-700 dark:bg-slate-900">
        <div className="border-b border-slate-200 px-4 py-3 text-sm font-semibold text-slate-800 dark:border-slate-700 dark:text-slate-100">
          {product.kind === 'document' ? 'Documents' : 'Tables'}
        </div>
        {datasets.length === 0 ? (
          <div className="px-4 py-6 text-sm text-slate-500 dark:text-slate-400">
            Nothing published yet. Add a file below.
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="text-left text-xs text-slate-500 dark:text-slate-400">
              <tr>
                <th className="px-4 py-2">{product.kind === 'document' ? 'Document' : 'Table'}</th>
                <th className="px-4 py-2">Source</th>
                <th className="px-4 py-2">Size</th>
                <th className="px-4 py-2">Snapshot</th>
                <th className="px-4 py-2" />
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100 dark:divide-slate-800">
              {datasets.map((d) => (
                <tr key={d.dataset_id}>
                  <td className="px-4 py-2 font-mono text-xs text-slate-800 dark:text-slate-100">
                    {d.table ?? d.query_path.split('/').pop()}
                  </td>
                  <td className="max-w-[260px] truncate px-4 py-2 text-xs text-slate-500" title={d.query_path}>
                    {d.query_path}
                  </td>
                  <td className="px-4 py-2 text-xs text-slate-500">{fmtBytes(d.size_bytes)}</td>
                  <td className="px-4 py-2 text-xs text-slate-500">
                    {d.snapshot_at ? new Date(d.snapshot_at).toLocaleString() : (
                      <span className="text-amber-600">not uploaded</span>
                    )}
                  </td>
                  <td className="px-4 py-2 text-right">
                    <button className="text-xs text-slate-400 hover:text-red-600" onClick={() => removeDataset(d)} title="Remove from product">
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <AddFile
        product={product}
        folders={localFolders}
        preselect={preselect}
        onPublished={async () => {
          await loadDatasets();
          await onChanged();
        }}
      />
    </div>
  );
}

// ── add file ──────────────────────────────────────────────────────────

function AddFile({
  product,
  folders,
  preselect,
  onPublished,
}: {
  product: Product;
  folders: string[];
  preselect: { folder: string; file: string } | null;
  onPublished: () => Promise<void>;
}) {
  const toast = useToast();
  const [folder, setFolder] = useState(preselect?.folder ?? folders[0] ?? '');
  const [files, setFiles] = useState<DatasetMetadataPayload[]>([]);
  const [file, setFile] = useState(preselect?.file ?? '');
  const [check, setCheck] = useState<Precheck | null>(null);
  const [table, setTable] = useState('');
  const [checking, setChecking] = useState(false);
  const [progress, setProgress] = useState<PublishProgressPayload | null>(null);
  const [publishing, setPublishing] = useState(false);

  const allowed = product.kind === 'document' ? DOCUMENT : TABULAR;

  useEffect(() => {
    if (!folder) return;
    let cancelled = false;
    invoke<DatasetMetadataPayload[]>('get_cached_folder_metadata', { folderPath: folder })
      .then((list) => {
        if (cancelled) return;
        setFiles(list.filter((d) => allowed.has(d.file_format.toLowerCase())).sort((a, b) => a.relative_path.localeCompare(b.relative_path)));
      })
      .catch(() => setFiles([]));
    return () => {
      cancelled = true;
    };
  }, [folder, allowed]);

  useEffect(() => {
    if (!folder || !file) {
      setCheck(null);
      return;
    }
    let cancelled = false;
    setChecking(true);
    invoke<Precheck>('publish_precheck', { folderPath: folder, relativePath: file })
      .then((c) => {
        if (cancelled) return;
        setCheck(c);
        setTable(c.suggested_table);
      })
      .catch((err) => {
        if (!cancelled) toast.error(`Could not read file: ${errText(err)}`);
      })
      .finally(() => !cancelled && setChecking(false));
    return () => {
      cancelled = true;
    };
  }, [folder, file, toast]);

  useEffect(() => {
    const un = listen<PublishProgressPayload>('publish_progress', (e) => {
      if (e.payload.product_hash === product.hash && e.payload.relative_path === file) {
        setProgress(e.payload);
      }
    });
    return () => {
      void un.then((f) => f());
    };
  }, [product.hash, file]);

  const publish = async () => {
    if (!folder || !file) return;
    setPublishing(true);
    setProgress(null);
    try {
      const out = await invoke<Published>('publish_file', {
        hash: product.hash,
        folderPath: folder,
        relativePath: file,
        table: product.kind === 'tabular' ? table || null : null,
      });
      toast.success(
        product.kind === 'document'
          ? `Published ${out.passages} passages from ${file}`
          : `Published ${file} as "${out.table}" (${fmtBytes(out.size_bytes)})`,
      );
      setFile('');
      setCheck(null);
      await onPublished();
    } catch (err) {
      toast.error(`Publish failed: ${errText(err)}`);
    } finally {
      setPublishing(false);
    }
  };

  const pct =
    progress && progress.total_bytes > 0
      ? Math.min(100, Math.round((progress.done_bytes / progress.total_bytes) * 100))
      : null;

  return (
    <div className="rounded-xl border border-slate-200 bg-white p-4 dark:border-slate-700 dark:bg-slate-900">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-800 dark:text-slate-100">
        <Upload className="h-4 w-4" />
        Add {product.kind === 'document' ? 'a document' : 'a table'}
      </h3>

      {folders.length === 0 ? (
        <p className="text-sm text-slate-500">Add a local folder as a source first.</p>
      ) : (
        <div className="grid gap-3 md:grid-cols-2">
          <label className="text-xs text-slate-600 dark:text-slate-300">
            Folder
            <select className={`mt-1 ${inputCls}`} value={folder} onChange={(e) => { setFolder(e.target.value); setFile(''); }}>
              {folders.map((f) => (
                <option key={f} value={f}>{f}</option>
              ))}
            </select>
          </label>
          <label className="text-xs text-slate-600 dark:text-slate-300">
            File
            <select className={`mt-1 ${inputCls}`} value={file} onChange={(e) => setFile(e.target.value)}>
              <option value="">Choose…</option>
              {files.map((d) => (
                <option key={d.relative_path} value={d.relative_path}>
                  {d.relative_path} · {fmtBytes(d.size_bytes)}
                </option>
              ))}
            </select>
          </label>
        </div>
      )}

      {checking && (
        <div className="mt-3 flex items-center gap-2 text-xs text-slate-500">
          <Loader2 className="h-3.5 w-3.5 animate-spin" /> Reading file…
        </div>
      )}

      {check && !checking && (
        <div className="mt-4 space-y-3">
          {check.kind === 'unsupported' && (
            <div className="rounded-md border border-red-200 bg-red-50 p-3 text-xs text-red-800">
              .{check.file_format} cannot be published as a dataset.
            </div>
          )}
          {check.pii_columns.length > 0 && (
            <div className="flex gap-2 rounded-md border border-amber-200 bg-amber-50 p-3 text-xs text-amber-800 dark:border-amber-900/50 dark:bg-amber-900/20 dark:text-amber-200">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <div>
                <div className="font-semibold">These columns look like personal data:</div>
                <div className="mt-0.5 font-mono">{check.pii_columns.join(', ')}</div>
                <div className="mt-1">
                  Once published, this data is hosted by Sery and queryable by anyone who pays. Remove these columns or make sure you have the right to sell them.
                </div>
              </div>
            </div>
          )}
          <div className="grid gap-3 md:grid-cols-3 text-xs text-slate-600 dark:text-slate-300">
            <div>
              <div className="text-slate-400">Size</div>
              <div className="font-medium text-slate-800 dark:text-slate-100">{fmtBytes(check.size_bytes)}</div>
            </div>
            <div>
              <div className="text-slate-400">{product.kind === 'document' ? 'Format' : 'Rows (est.)'}</div>
              <div className="font-medium text-slate-800 dark:text-slate-100">
                {product.kind === 'document' ? check.file_format : (check.row_count_estimate?.toLocaleString() ?? '—')}
              </div>
            </div>
            <div>
              <div className="text-slate-400">Columns</div>
              <div className="font-medium text-slate-800 dark:text-slate-100">{check.columns.length}</div>
            </div>
          </div>
          {product.kind === 'tabular' && (
            <label className="block text-xs text-slate-600 dark:text-slate-300">
              Table name buyers will query
              <input className={`mt-1 font-mono ${inputCls}`} value={table} onChange={(e) => setTable(e.target.value)} />
              <span className="mt-1 block text-[11px] text-slate-400">
                Buyers write <code>SELECT … FROM &quot;{table || check.suggested_table}&quot;</code>. Your file path is never shown.
              </span>
            </label>
          )}
          {check.columns.length > 0 && (
            <details className="text-xs">
              <summary className="cursor-pointer text-slate-500">Columns buyers will see</summary>
              <div className="mt-2 flex flex-wrap gap-1">
                {check.columns.map((c) => (
                  <span key={c.name} className={`rounded px-1.5 py-0.5 font-mono ${check.pii_columns.includes(c.name) ? 'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-200' : 'bg-slate-100 text-slate-700 dark:bg-slate-800 dark:text-slate-200'}`}>
                    {c.name} <span className="text-slate-400">{c.type}</span>
                  </span>
                ))}
              </div>
            </details>
          )}

          {progress && progress.stage !== 'done' && progress.stage !== 'error' && (
            <div className="text-xs text-slate-500">
              <div className="flex justify-between">
                <span className="capitalize">{progress.stage}…</span>
                {pct !== null && <span>{pct}%</span>}
              </div>
              <div className="mt-1 h-1.5 w-full overflow-hidden rounded bg-slate-100 dark:bg-slate-800">
                <div className="h-full bg-purple-600 transition-all" style={{ width: `${pct ?? 10}%` }} />
              </div>
            </div>
          )}

          <div className="flex justify-end">
            <button className={primaryCls} disabled={publishing || check.kind === 'unsupported'} onClick={publish}>
              {publishing ? <Loader2 className="h-4 w-4 animate-spin" /> : <Upload className="h-4 w-4" />}
              {publishing ? 'Publishing…' : 'Publish'}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
