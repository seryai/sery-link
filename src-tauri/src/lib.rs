mod audit;
mod auth;
mod commands;
mod config;
mod csv;
mod duckdb_engine;
mod error;
mod events;
mod excel;
mod export_import;
mod fleet;
mod history;
mod keyring_store;
mod metadata_cache;
mod pairing;
mod plugin;
mod plugin_marketplace;
mod plugin_runtime;
mod recipe_executor;
mod relationship_detector;
mod scanner;
mod schema_diff;
mod stats;
mod tray;
mod watcher;
mod websocket;

use tauri::{image::Image, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
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
            commands::pair_request,
            commands::pair_status,
            commands::pair_complete,
            commands::list_fleet,
            commands::get_config,
            commands::save_config,
            commands::add_watched_folder,
            commands::remove_watched_folder,
            commands::scan_folder,
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
            commands::show_main_window,
            commands::set_launch_at_login,
            commands::search_cached_datasets,
            commands::get_all_cached_datasets,
            commands::get_cached_dataset,
            commands::upsert_cached_dataset,
            commands::upsert_cached_datasets,
            commands::clear_cached_workspace,
            commands::get_cache_stats,
            commands::detect_dataset_relationships,
            commands::export_configuration,
            commands::import_configuration,
            commands::validate_import_file,
            commands::read_file,
            commands::list_plugins,
            commands::enable_plugin,
            commands::disable_plugin,
            commands::uninstall_plugin,
            commands::load_plugin_into_runtime,
            commands::unload_plugin_from_runtime,
            commands::is_plugin_loaded,
            commands::get_loaded_plugins,
            commands::execute_plugin_with_file,
            commands::load_marketplace,
            commands::search_marketplace,
            commands::get_featured_plugins,
            commands::get_popular_plugins,
            commands::get_marketplace_plugin,
            commands::install_marketplace_plugin,
            commands::load_recipes_from_dir,
            commands::load_recipe,
            commands::search_recipes,
            commands::get_recipe,
            commands::list_recipes,
            commands::filter_recipes_by_data_source,
            commands::render_recipe_sql,
            commands::validate_recipe_tables,
            commands::execute_recipe,
            commands::get_current_auth_mode,
            commands::check_feature_available,
            commands::set_auth_mode,
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
