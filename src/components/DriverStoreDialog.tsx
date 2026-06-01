// Driver Store — on-demand installation of Java JDBC drivers for enterprise
// databases (Oracle, Snowflake, DB2, SAP HANA, Teradata, …).
//
// Drivers are stored at ~/.seryai/drivers/ (JREs + JARs, ~180 MB per JRE).
// The registry is fetched from GitHub on first open; a local-only snapshot
// is shown immediately so the UI renders without a network round-trip.
//
// Two tabs:
//   Drivers — list of available drivers with Install/Uninstall/Update buttons.
//   Java Runtime — Managed (default) / System / Custom path selector.

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Download, Loader2, RefreshCw, X } from 'lucide-react';
import { SourceIcon } from './SourceIcon';
import { useToast } from './Toast';

// ──────────── Type definitions ────────────

interface DriverStatus {
  db_type: string;
  label: string;
  version: string;
  size: number;
  installed: boolean;
  installed_version?: string;
  update_available: boolean;
  jre: string;
  jre_installed: boolean;
}

interface DriverStoreUsage {
  total_bytes: number;
  jre_bytes: number;
  driver_bytes: number;
  jres: Array<{ id: string; bytes: number }>;
  drivers: Array<{ id: string; bytes: number }>;
}

interface JavaRuntimeConfig {
  mode: 'managed' | 'system' | 'custom';
  custom_java_path?: string | null;
}

interface DriverInstallProgressPayload {
  db_type: string;
  step: string;
  downloaded?: number;
  total?: number;
}

// ──────────── Helpers ────────────

function formatBytes(bytes: number): string {
  if (!bytes) return '';
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

// ──────────── Sub-components ────────────

function StatusBadge({ installed, updateAvailable }: { installed: boolean; updateAvailable?: boolean }) {
  if (updateAvailable) {
    return (
      <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-[11px] text-amber-600">
        Update
      </span>
    );
  }
  if (installed) {
    return (
      <span className="rounded-full bg-emerald-500/15 px-2 py-0.5 text-[11px] text-emerald-600">
        Installed
      </span>
    );
  }
  return (
    <span className="rounded-full bg-muted px-2 py-0.5 text-[11px] text-muted-foreground">
      Available
    </span>
  );
}

function ProgressBar({ downloaded, total }: { downloaded?: number; total?: number }) {
  if (!total) return <Loader2 className="h-3.5 w-3.5 animate-spin" />;
  const pct = Math.round(((downloaded ?? 0) / total) * 100);
  return (
    <div className="flex items-center gap-2 w-28">
      <div className="flex-1 h-1.5 rounded-full bg-muted overflow-hidden">
        <div className="h-full bg-primary transition-all" style={{ width: `${pct}%` }} />
      </div>
      <span className="text-[10px] text-muted-foreground shrink-0">{pct}%</span>
    </div>
  );
}

// ──────────── Main dialog ────────────

interface DriverStoreDialogProps {
  /** When true the panel is rendered; false = unmounted (not just hidden). */
  open: boolean;
  onClose: () => void;
  /**
   * When true, render as an embedded panel without the modal overlay.
   * Use this when hosting the component inside a parent panel (e.g. Settings).
   */
  embedded?: boolean;
}

export function DriverStoreDialog({ open, onClose, embedded = false }: DriverStoreDialogProps) {
  const toast = useToast();

  const [tab, setTab] = useState<'drivers' | 'runtime'>('drivers');
  const [drivers, setDrivers] = useState<DriverStatus[]>([]);
  const [usage, setUsage] = useState<DriverStoreUsage | null>(null);
  const [javaConfig, setJavaConfig] = useState<JavaRuntimeConfig>({ mode: 'managed' });
  const [customPath, setCustomPath] = useState('');
  const [savingRuntime, setSavingRuntime] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [installing, setInstalling] = useState<string | null>(null);
  const [progress, setProgress] = useState<DriverInstallProgressPayload | null>(null);

  const unlistenRef = useRef<(() => void) | null>(null);

  // ── Load data ────────────────────────────

  async function loadDriversLocal() {
    try {
      const result = await invoke<DriverStatus[]>('list_drivers_local');
      setDrivers(result);
    } catch (e) {
      // ignore — will be populated by full refresh
    }
  }

  async function loadDriversFull() {
    try {
      const result = await invoke<DriverStatus[]>('list_drivers');
      setDrivers(result);
    } catch (e) {
      // network unavailable — local snapshot stays
    }
  }

  async function loadUsage() {
    try {
      const result = await invoke<DriverStoreUsage>('get_driver_store_usage');
      setUsage(result);
    } catch {
      setUsage(null);
    }
  }

  async function loadJavaConfig() {
    try {
      const result = await invoke<JavaRuntimeConfig>('get_java_runtime_config');
      setJavaConfig(result);
      setCustomPath(result.custom_java_path ?? '');
    } catch {
      // ignore
    }
  }

  // ── Actions ────────────────────────────

  async function handleInstall(dbType: string) {
    if (installing) return;
    setInstalling(dbType);
    setProgress(null);
    try {
      await invoke('install_driver_cmd', { dbType });
      await loadDriversFull();
      await loadUsage();
      const label = drivers.find((d) => d.db_type === dbType)?.label ?? dbType;
      toast.info(`${label} driver installed successfully.`);
    } catch (e) {
      const label = drivers.find((d) => d.db_type === dbType)?.label ?? dbType;
      toast.info(`Failed to install ${label}: ${e}`);
    } finally {
      setInstalling(null);
      setProgress(null);
    }
  }

  async function handleUninstall(dbType: string) {
    const label = drivers.find((d) => d.db_type === dbType)?.label ?? dbType;
    try {
      await invoke('uninstall_driver_cmd', { dbType });
      await loadDriversFull();
      await loadUsage();
      toast.info(`${label} driver uninstalled.`);
    } catch (e) {
      toast.info(`Failed to uninstall ${label}: ${e}`);
    }
  }

  async function handleForceRefresh() {
    setRefreshing(true);
    try {
      await invoke('invalidate_driver_registry_cache');
      await loadDriversFull();
      await loadUsage();
    } finally {
      setRefreshing(false);
    }
  }

  async function handleSaveRuntime() {
    setSavingRuntime(true);
    try {
      const payload: JavaRuntimeConfig = {
        mode: javaConfig.mode,
        custom_java_path: javaConfig.mode === 'custom' ? customPath.trim() || null : null,
      };
      const saved = await invoke<JavaRuntimeConfig>('set_java_runtime_config', { config: payload });
      setJavaConfig(saved);
      setCustomPath(saved.custom_java_path ?? '');
      toast.info('Java runtime configuration saved.');
    } catch (e) {
      toast.info(`Failed to save Java runtime: ${e}`);
    } finally {
      setSavingRuntime(false);
    }
  }

  // ── Lifecycle ────────────────────────────

  useEffect(() => {
    if (!open) return;

    loadDriversLocal();
    loadJavaConfig();
    loadUsage();
    loadDriversFull();

    listen<DriverInstallProgressPayload>('driver-install-progress', (event) => {
      const payload = event.payload;
      if (payload.step === 'done') {
        setProgress(null);
      } else {
        setProgress(payload);
      }
    }).then((unlisten) => {
      unlistenRef.current = unlisten;
    });

    return () => {
      unlistenRef.current?.();
      unlistenRef.current = null;
    };
  }, [open]);

  if (!open) return null;

  const updatableCount = drivers.filter((d) => d.update_available).length;

  const inner = (
    <div className={embedded ? 'flex flex-col' : 'relative flex h-[80vh] w-[760px] max-w-[95vw] flex-col rounded-xl border bg-background shadow-2xl'}>
      {/* Header — only in modal mode */}
      {!embedded && (
        <div className="flex items-center justify-between border-b px-6 py-4">
          <div>
            <h2 className="text-base font-semibold">Driver Store</h2>
            <p className="text-xs text-muted-foreground mt-0.5">
              Install Java JDBC drivers for Oracle, Snowflake, DB2, SAP HANA, and 25+ more.
            </p>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-muted-foreground hover:bg-muted transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
      )}

        {/* Usage summary bar */}
        {usage && (
          <div className="flex items-center gap-4 border-b bg-muted/20 px-6 py-2 text-xs text-muted-foreground">
            <span>
              Total: <strong>{formatBytes(usage.total_bytes)}</strong>
            </span>
            <span>JREs: {formatBytes(usage.jre_bytes)}</span>
            <span>Drivers: {formatBytes(usage.driver_bytes)}</span>
          </div>
        )}

        {/* Tab bar */}
        <div className="flex items-center justify-between border-b px-6 pt-3">
          <div className="flex gap-1">
            {(['drivers', 'runtime'] as const).map((t) => (
              <button
                key={t}
                onClick={() => setTab(t)}
                className={`rounded-t px-3 py-1.5 text-sm font-medium transition-colors ${
                  tab === t
                    ? 'border-b-2 border-primary text-foreground'
                    : 'text-muted-foreground hover:text-foreground'
                }`}
              >
                {t === 'drivers' ? (
                  <span className="flex items-center gap-1.5">
                    Drivers
                    {updatableCount > 0 && (
                      <span className="h-2 w-2 rounded-full bg-amber-500 inline-block" />
                    )}
                  </span>
                ) : (
                  'Java Runtime'
                )}
              </button>
            ))}
          </div>
          {tab === 'drivers' && (
            <button
              onClick={handleForceRefresh}
              disabled={refreshing}
              className="flex items-center gap-1 rounded-full px-2.5 py-1 text-xs text-muted-foreground hover:bg-muted transition-colors disabled:opacity-50"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? 'animate-spin' : ''}`} />
              Refresh
            </button>
          )}
        </div>

        {/* Scrollable content */}
        <div className="flex-1 overflow-y-auto px-6 py-4">
          {tab === 'drivers' && (
            <div className="space-y-1">
              {drivers.length === 0 && (
                <p className="py-12 text-center text-sm text-muted-foreground">Loading drivers…</p>
              )}
              {drivers.map((driver) => {
                const isThisInstalling =
                  installing === driver.db_type ||
                  (progress?.db_type === driver.db_type && progress.step !== 'done');
                return (
                  <div
                    key={driver.db_type}
                    className="flex items-center gap-3 rounded-lg px-3 py-2.5 hover:bg-muted/40 transition-colors"
                  >
                    <div className="flex h-8 w-8 shrink-0 items-center justify-center">
                      <SourceIcon kind={driver.db_type} size="sm" />
                    </div>

                    {/* Label + meta */}
                    <div className="min-w-0 flex-1">
                      <div className="text-sm font-medium">{driver.label}</div>
                      <div className="flex items-center gap-2 mt-0.5">
                        <span className="text-[11px] text-muted-foreground">
                          JRE {driver.jre}
                        </span>
                        {driver.installed && driver.installed_version && (
                          <span className="text-[11px] text-muted-foreground">
                            v{driver.installed_version}
                          </span>
                        )}
                        {!driver.installed && driver.version && (
                          <span className="text-[11px] text-muted-foreground">
                            v{driver.version}
                          </span>
                        )}
                        {driver.size > 0 && (
                          <span className="text-[11px] text-muted-foreground">
                            {formatBytes(driver.size)}
                          </span>
                        )}
                      </div>
                    </div>

                    {/* Status + actions */}
                    <div className="flex shrink-0 items-center gap-2">
                      <StatusBadge installed={driver.installed} updateAvailable={driver.update_available} />

                      {isThisInstalling ? (
                        <ProgressBar
                          downloaded={progress?.downloaded}
                          total={progress?.total}
                        />
                      ) : driver.installed && !driver.update_available ? (
                        <button
                          onClick={() => handleUninstall(driver.db_type)}
                          disabled={installing !== null}
                          className="rounded-full px-2.5 py-1 text-xs text-muted-foreground hover:text-destructive hover:bg-muted transition-colors disabled:opacity-40"
                        >
                          Uninstall
                        </button>
                      ) : (
                        <button
                          onClick={() => handleInstall(driver.db_type)}
                          disabled={installing !== null}
                          className="flex items-center gap-1 rounded-full bg-primary px-2.5 py-1 text-xs text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-40"
                        >
                          <Download className="h-3 w-3" />
                          {driver.update_available ? 'Update' : 'Install'}
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {tab === 'runtime' && (
            <div className="space-y-6 max-w-md">
              <div className="space-y-3">
                <label className="text-sm font-medium">Java Runtime Mode</label>
                <div className="space-y-2">
                  {(['managed', 'system', 'custom'] as const).map((mode) => (
                    <label key={mode} className="flex items-start gap-3 cursor-pointer">
                      <input
                        type="radio"
                        name="java-mode"
                        value={mode}
                        checked={javaConfig.mode === mode}
                        onChange={() => setJavaConfig((c) => ({ ...c, mode }))}
                        className="mt-0.5"
                      />
                      <div>
                        <div className="text-sm font-medium capitalize">{mode}</div>
                        <div className="text-xs text-muted-foreground">
                          {mode === 'managed' && 'Sery Link downloads and manages the JRE automatically.'}
                          {mode === 'system' &&
                            'Use the Java binary found on your PATH (must be Java 11+).'}
                          {mode === 'custom' && 'Specify the exact path to your java executable or JAVA_HOME.'}
                        </div>
                      </div>
                    </label>
                  ))}
                </div>

                {javaConfig.mode === 'custom' && (
                  <input
                    type="text"
                    autoCorrect="off"
                    autoCapitalize="off"
                    spellCheck={false}
                    value={customPath}
                    onChange={(e) => setCustomPath(e.target.value)}
                    placeholder="/usr/local/opt/openjdk@21/bin/java"
                    className="w-full rounded-md border bg-background px-3 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-ring"
                  />
                )}

                <button
                  onClick={handleSaveRuntime}
                  disabled={savingRuntime || (javaConfig.mode === 'custom' && !customPath.trim())}
                  className="flex items-center gap-1.5 rounded-full bg-primary px-4 py-1.5 text-sm text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-40"
                >
                  {savingRuntime && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                  Save
                </button>
              </div>

              {/* JRE disk usage */}
              {usage && usage.jres.length > 0 && (
                <div className="space-y-2">
                  <div className="text-sm font-medium">Installed JREs</div>
                  {usage.jres.map((jre) => (
                    <div
                      key={jre.id}
                      className="flex items-center justify-between rounded-lg border bg-muted/20 px-3 py-2 text-sm"
                    >
                      <span>JRE {jre.id}</span>
                      <span className="text-xs text-muted-foreground">{formatBytes(jre.bytes)}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
  );

  if (embedded) return inner;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      {inner}
    </div>
  );
}
