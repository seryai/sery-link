//! System tray — icon, menu, and state machine.
//!
//! The tray is the agent's persistent UI: even when the main window is hidden,
//! users can see the agent's status at a glance and reach the most common
//! actions (show window, pause sync, open the web app, quit).
//!
//! State is driven by `set_state(app, "online"|"syncing"|"offline"|"error")`
//! which updates the tooltip and the status line at the top of the menu.
//! Icon variants are a future enhancement — for now we reuse the single
//! bundled PNG and rely on the tooltip/menu for at-a-glance status.

use crate::config::Config;
use crate::stats;
use once_cell::sync::Lazy;
use std::sync::{Arc, RwLock};
use tauri::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

// ---------------------------------------------------------------------------
// Menu item IDs — keep in sync with the match arms in `on_menu_event`.
// ---------------------------------------------------------------------------

const MI_STATUS: &str = "status_header";
const MI_QUERIES_TODAY: &str = "stats_today";
const MI_SHOW: &str = "show_window";
const MI_HIDE: &str = "hide_window";
const MI_PAUSE_SYNC: &str = "pause_sync";
const MI_RESUME_SYNC: &str = "resume_sync";
const MI_OPEN_WEB: &str = "open_web";
const MI_QUIT: &str = "quit";

// ---------------------------------------------------------------------------
// Global tray handle — tray events come in on background threads so we need
// a cross-thread-safe handle to update state from anywhere in the app.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct TrayState {
    connection: String, // "online" | "syncing" | "offline" | "error"
    paused: bool,
}

static TRAY_STATE: Lazy<Arc<RwLock<TrayState>>> = Lazy::new(|| {
    Arc::new(RwLock::new(TrayState {
        connection: "offline".to_string(),
        paused: false,
    }))
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build the tray icon, menu and event handlers. Called once from `lib.rs`
/// during app setup.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build_menu(app, &TrayState::default())?;

    let _tray = TrayIconBuilder::with_id("main")
        .tooltip("Sery Link — starting…")
        .icon(app.default_window_icon().cloned().unwrap())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu_event(app, event))
        .on_tray_icon_event(|tray, event| {
            // Left click toggles the main window — classic menubar app behaviour.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// Update the tray's connection state. Called whenever the WebSocket status
/// changes or a sync completes.
pub fn set_state<R: Runtime>(app: &AppHandle<R>, state: &str) {
    if let Ok(mut s) = TRAY_STATE.write() {
        s.connection = state.to_string();
    }
    refresh(app);
}

/// Rebuild the tray menu with current state + stats. Safe to call from any
/// thread.
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let state = TRAY_STATE.read().map(|s| s.clone()).unwrap_or_default();

    // Rebuild the menu so the status header and "queries today" line reflect
    // the latest snapshot. Tauri 2's menu API doesn't let us mutate individual
    // items from arbitrary threads, so a full rebuild is the simplest path.
    if let Some(tray) = app.tray_by_id("main") {
        if let Ok(menu) = build_menu(app, &state) {
            let _ = tray.set_menu(Some(menu));
        }
        let _ = tray.set_tooltip(Some(tooltip_for(&state)));
    }
}

// ---------------------------------------------------------------------------
// Menu construction
// ---------------------------------------------------------------------------

fn build_menu<R: Runtime>(app: &AppHandle<R>, state: &TrayState) -> tauri::Result<Menu<R>> {
    let stats_snapshot = stats::load().unwrap_or_default();

    // Status header — disabled so it looks like a label. Shows current
    // connection state with a leading dot so users can scan it quickly.
    let status_label = format!("{} {}", status_dot(&state.connection), status_text(state));
    let status = MenuItem::with_id(app, MI_STATUS, status_label, false, None::<&str>)?;

    // Stats line — also a disabled label.
    let stats_label = format!(
        "{} queries today · {} total",
        stats_snapshot.queries_today, stats_snapshot.total_queries
    );
    let stats_item = MenuItem::with_id(app, MI_QUERIES_TODAY, stats_label, false, None::<&str>)?;

    let sep1 = PredefinedMenuItem::separator(app)?;

    let show = MenuItem::with_id(app, MI_SHOW, "Show Sery Link", true, Some("CmdOrCtrl+1"))?;
    let hide = MenuItem::with_id(app, MI_HIDE, "Hide Window", true, None::<&str>)?;

    let sep2 = PredefinedMenuItem::separator(app)?;

    // Toggle between Pause and Resume depending on current state.
    let sync_toggle = if state.paused {
        MenuItem::with_id(app, MI_RESUME_SYNC, "Resume Syncing", true, None::<&str>)?
    } else {
        MenuItem::with_id(app, MI_PAUSE_SYNC, "Pause Syncing", true, None::<&str>)?
    };

    let open_web = MenuItem::with_id(app, MI_OPEN_WEB, "Open Sery in Browser", true, None::<&str>)?;

    let sep3 = PredefinedMenuItem::separator(app)?;

    let quit = MenuItem::with_id(app, MI_QUIT, "Quit Sery Link", true, Some("CmdOrCtrl+Q"))?;

    Menu::with_items(
        app,
        &[
            &status,
            &stats_item,
            &sep1,
            &show,
            &hide,
            &sep2,
            &sync_toggle,
            &open_web,
            &sep3,
            &quit,
        ],
    )
}

fn status_dot(state: &str) -> &'static str {
    // Unicode dots — keeps things lightweight without bundling extra icons.
    match state {
        "online" => "●",
        "syncing" => "◐",
        "error" => "●",
        _ => "○",
    }
}

fn status_text(state: &TrayState) -> String {
    if state.paused {
        return "Sync paused".to_string();
    }
    match state.connection.as_str() {
        "online" => "Connected".to_string(),
        "syncing" => "Syncing…".to_string(),
        "connecting" => "Connecting…".to_string(),
        "error" => "Connection error".to_string(),
        _ => "Offline".to_string(),
    }
}

fn tooltip_for(state: &TrayState) -> String {
    let base = status_text(state);
    format!("Sery Link — {}", base)
}

// ---------------------------------------------------------------------------
// Event handlers
// ---------------------------------------------------------------------------

fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id.as_ref() {
        MI_SHOW => show_main_window(app),
        MI_HIDE => hide_main_window(app),
        MI_PAUSE_SYNC => set_paused(app, true),
        MI_RESUME_SYNC => set_paused(app, false),
        MI_OPEN_WEB => {
            if let Ok(config) = Config::load() {
                let url = config.cloud.web_url.clone();
                let _ = open::that(url);
            }
        }
        MI_QUIT => {
            app.exit(0);
        }
        _ => {}
    }
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }

    // On macOS bring the dock icon back while the window is visible.
    #[cfg(target_os = "macos")]
    {
        use tauri::ActivationPolicy;
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
    }
}

fn hide_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    #[cfg(target_os = "macos")]
    {
        use tauri::ActivationPolicy;
        let _ = app.set_activation_policy(ActivationPolicy::Accessory);
    }
}

fn toggle_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        match window.is_visible() {
            Ok(true) => hide_main_window(app),
            _ => show_main_window(app),
        }
    }
}

fn set_paused<R: Runtime>(app: &AppHandle<R>, paused: bool) {
    if let Ok(mut s) = TRAY_STATE.write() {
        s.paused = paused;
    }
    // Flip auto_sync_on_change in config so the watcher matches the menu state.
    if let Ok(mut config) = Config::load() {
        config.sync.auto_sync_on_change = !paused;
        let _ = config.save();
    }
    // Kick the watcher off/on asynchronously.
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if paused {
            let _ = crate::commands::stop_file_watcher().await;
        } else {
            let _ = crate::commands::start_file_watcher().await;
        }
        refresh(&app_clone);
    });
}
