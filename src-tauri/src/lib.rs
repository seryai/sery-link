mod audit;
mod auth;
mod commands;
mod config;
mod csv;
mod deep_link;
mod duckdb_engine;
mod error;
mod events;
mod excel;
mod export_import;
mod disk_space;
mod gdrive_api;
mod gdrive_cache;
mod gdrive_creds;
mod gdrive_oauth;
mod gdrive_refresh;
mod gdrive_skipped;
mod gdrive_walker;
mod machines;
mod history;
mod hotkey;
mod keyring_store;
mod mcp;
mod metadata_cache;
mod relationship_detector;
mod remote;
mod remote_creds;
mod scan_cache;
mod scanner;
mod url;
mod schema_diff;
mod schema_notifications;
mod stats;
mod tray;
mod watcher;
mod websocket;

use tauri::{image::Image, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ── MCP stdio mode dispatch ─────────────────────────────────
    //
    // When the LLM client (Claude Desktop / Cursor / Zed / …) spawns
    // us with `--mcp-stdio --root <dir>`, we don't start the Tauri
    // GUI at all — we hand control to `mcp::run_stdio()` which serves
    // the same six tools exposed by the standalone `sery-mcp` binary
    // (it's the same library underneath).
    //
    // Detection happens BEFORE Tauri builder so we never open an
    // empty window or steal focus from the spawning process. The
    // user's `mcp.json` config typically looks like:
    //
    //   {
    //     "mcpServers": {
    //       "sery": {
    //         "command": "/Applications/Sery Link.app/.../sery-link",
    //         "args": ["--mcp-stdio", "--root", "/Users/me/Documents"]
    //       }
    //     }
    //   }
    //
    // For users who want only the MCP bridge without the GUI, the
    // standalone `cargo install sery-mcp` is still the right answer.
    let cli_args: Vec<String> = std::env::args().collect();
    if cli_args.iter().any(|a| a == "--mcp-stdio") {
        match mcp::parse_stdio_args(&cli_args) {
            Some(root) => {
                if let Err(e) = mcp::run_stdio(root) {
                    eprintln!("sery-link MCP stdio server exited with error: {e:#}");
                    std::process::exit(1);
                }
                return;
            }
            None => {
                eprintln!(
                    "--mcp-stdio requires --root <path>\n\
                     Example: sery-link --mcp-stdio --root /Users/me/Documents"
                );
                std::process::exit(2);
            }
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .setup(|app| {
            // Set the window icon explicitly so it shows in dev mode too.
            // On macOS, the dock icon comes from the bundle .icns file, but
            // setting window.set_icon() ensures the icon shows in dev mode
            // and on minimize badges.
            if let Some(window) = app.get_webview_window("main") {
                let icon = Image::from_bytes(include_bytes!("../icons/window-icon.png"))
                    .expect("failed to load app icon");
                let _ = window.set_icon(icon);
            }

            // Stash a global AppHandle so background tasks (watcher, periodic
            // rescan) can emit events without us threading it through every
            // layer of the call graph.
            events::set_app_handle(app.handle().clone());

            // Build the system tray (icons, menu, event handlers) and stash a
            // handle so we can mutate icon state from other threads as the
            // agent transitions between online/syncing/offline/error.
            tray::build(app.handle())?;

            // Record agent uptime start so stats.json reflects this session.
            let _ = stats::record_startup();

            // On macOS, transition to Accessory activation policy when the
            // window is hidden so the dock icon disappears; come back to
            // Regular when the window is visible. This makes the agent feel
            // like a first-class menubar app.
            #[cfg(target_os = "macos")]
            {
                use tauri::ActivationPolicy;
                app.set_activation_policy(ActivationPolicy::Regular);
            }

            // Migrate existing users to the new auth mode system (v0.4.0+)
            if let Ok(mut config) = config::Config::load() {
                let _ = config.migrate_if_needed();
                apply_autostart(app.handle(), config.app.launch_at_login);
            }

            // Register the Quick-Ask global hotkey (ROADMAP F9). Failures
            // are non-fatal — the hotkey is a UX nicety, not a core
            // dependency.
            if let Err(err) = hotkey::register(app.handle()) {
                eprintln!("[setup] could not register Quick-Ask hotkey: {err}");
            }

            // ROADMAP F3 / F1 — `seryai://` URL-scheme dispatcher. Routes
            // `seryai://reveal?path=…` to OS-native file reveal and
            // (placeholder) `seryai://pair?key=…` to a frontend event the
            // join-existing-workspace UI can later listen for.
            //
            // tauri-plugin-deep-link emits these events on the main app
            // handle; we forward each URL to deep_link::handle_url which
            // does the actual dispatch. Failures are logged + swallowed
            // so a bad URL doesn't crash the app.
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let app_handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        deep_link::handle_url(&app_handle, url.as_str());
                    }
                });
            }

            // Hourly background refresh of every watched Drive folder.
            // Skips silently when no folders are watched or when the
            // user is disconnected; logs per-folder failures to stderr
            // without bothering the user.
            gdrive_refresh::start_refresh_loop(app.handle().clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            // Intercept the window close button: hide instead of quit so the
            // WebSocket tunnel and file watcher keep running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();

                // First-time close: surface a notification so the user knows
                // the app is still running in the tray. Only fires once.
                if let Ok(mut config) = config::Config::load() {
                    if !config.app.window_hide_notified {
                        config.app.window_hide_notified = true;
                        let _ = config.save();
                        events::notify_window_hidden(window.app_handle());
                    }
                }

                // On macOS, drop the dock icon when the window hides.
                #[cfg(target_os = "macos")]
                {
                    use tauri::ActivationPolicy;
                    let _ = window
                        .app_handle()
                        .set_activation_policy(ActivationPolicy::Accessory);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_auth_flow,
            commands::auth_with_key,
            commands::bootstrap_workspace,
            commands::list_machines,
            commands::get_config,
            commands::save_config,
            commands::add_watched_folder,
            commands::add_remote_source,
            commands::remove_watched_folder,
            commands::set_folder_mcp_enabled,
            commands::get_mcp_snippets,
            commands::scan_folder,
            commands::search_all_folders,
            commands::profile_dataset,
            commands::read_dataset_rows,
            commands::convert_to_parquet,
            commands::get_cached_folder_metadata,
            commands::rescan_folder,
            commands::sync_metadata,
            commands::has_token,
            commands::get_agent_info,
            commands::logout,
            commands::start_websocket_tunnel,
            commands::get_websocket_status,
            commands::start_file_watcher,
            commands::stop_file_watcher,
            commands::restart_file_watcher,
            commands::get_query_history,
            commands::clear_query_history,
            commands::get_stats,
            commands::get_sync_audit,
            commands::clear_sync_audit,
            commands::clear_cloud_metadata,
            commands::export_diagnostic_bundle,
            commands::open_in_sery_cloud,
            commands::complete_first_run,
            commands::reveal_in_finder,
            commands::reveal_audit_file_in_finder,
            commands::show_main_window,
            commands::set_launch_at_login,
            commands::search_cached_datasets,
            commands::get_all_cached_datasets,
            commands::get_cached_dataset,
            commands::upsert_cached_dataset,
            commands::upsert_cached_datasets,
            commands::clear_cached_workspace,
            commands::get_cache_stats,
            commands::compute_schema_diff,
            commands::get_schema_notifications,
            commands::mark_schema_notification_read,
            commands::mark_all_schema_notifications_read,
            commands::clear_schema_notifications,
            commands::detect_dataset_relationships,
            commands::export_configuration,
            commands::import_configuration,
            commands::validate_import_file,
            commands::read_file,
            commands::get_current_auth_mode,
            commands::check_feature_available,
            commands::set_auth_mode,
            commands::set_local_only_mode,
            commands::is_local_only_mode_enabled,
            commands::fetch_workspace_recipes,
            commands::open_recipe_in_browser,
            commands::mark_recipe_run,
            // BYOK (Anthropic / OpenAI / Gemini) was removed in the
            // v0.5.3 → file-manager pivot. AI now happens cloud-side
            // via the dashboard / api server. See PR #62 (or git
            // log near 2026-05-03) for the removal.
            // Phase 3b — Google Drive OAuth (datalake/SETUP_GOOGLE_OAUTH.md).
            // Browser-based OAuth flow with PKCE; tokens land in OS
            // keychain via gdrive_creds. UI for these commands ships
            // in Phase 3c.
            commands::start_gdrive_oauth,
            commands::gdrive_status,
            commands::disconnect_gdrive,
            commands::gdrive_list_root_folders,
            commands::gdrive_list_folder,
            commands::gdrive_watch_folder,
            commands::gdrive_unwatch_folder,
            commands::gdrive_list_watched_folders,
            // Storage observability + cache cleanup. Independent of
            // OAuth state — clear_gdrive_cache leaves tokens intact
            // so the user can free disk without losing their grant.
            commands::get_storage_info,
            commands::clear_gdrive_cache,
            commands::get_gdrive_skipped,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Apply the autostart setting by enabling/disabling the Tauri autostart
/// plugin's underlying OS-level launcher (Launch Agent on macOS, registry
/// on Windows, desktop entry on Linux).
fn apply_autostart(app: &tauri::AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let currently = manager.is_enabled().unwrap_or(false);
    if enabled && !currently {
        let _ = manager.enable();
    } else if !enabled && currently {
        let _ = manager.disable();
    }
}
