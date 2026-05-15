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
    image::Image,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

static BASE_ICON: &[u8] = include_bytes!("../icons/tray-44x44.png");

/// True when macOS is running in dark mode (dark menu bar).
/// On non-macOS platforms always returns false.
fn is_dark_mode() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .eq_ignore_ascii_case("dark")
            })
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    false
}

/// Draw a small filled circle badge in the bottom-right corner of the tray
/// icon. Returns raw PNG bytes. Returns `None` for offline (use plain template).
///
/// In dark mode the base icon pixels are inverted (black → white) so the
/// Sery logo remains visible against the dark menu bar.
fn badge_icon_bytes(state: &str) -> Option<Vec<u8>> {
    let color: [u8; 4] = match state {
        "online" => [52, 211, 153, 255],                 // emerald-400
        "syncing" | "connecting" => [251, 191, 36, 255], // amber-400
        "error" => [248, 113, 113, 255],                 // rose-400
        _ => return None,
    };

    let dark = is_dark_mode();
    let mut img = image::load_from_memory(BASE_ICON).ok()?.to_rgba8();
    let (w, h) = img.dimensions();

    // Invert dark pixels for dark mode so the icon stays visible.
    // Template icons are black-on-transparent; inverting gives white-on-transparent.
    if dark {
        for pixel in img.pixels_mut() {
            let [r, g, b, a] = pixel.0;
            if a > 32 {
                *pixel = image::Rgba([255 - r, 255 - g, 255 - b, a]);
            }
        }
    }

    let r = 7i32;
    let cx = w as i32 - r - 2;
    let cy = h as i32 - r - 2;
    // Badge border color: dark in light mode, dark in dark mode (contrast against white base)
    let border: [u8; 4] = if dark { [30, 30, 30, 200] } else { [255, 255, 255, 220] };

    for py in 0..h {
        for px in 0..w {
            let dx = px as i32 - cx;
            let dy = py as i32 - cy;
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(px, py, image::Rgba(color));
            } else if dx * dx + dy * dy <= (r + 1) * (r + 1) {
                img.put_pixel(px, py, image::Rgba(border));
            }
        }
    }

    let mut buf = Vec::new();
    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(img.as_raw(), w, h, image::ColorType::Rgba8)
        .ok()?;
    Some(buf)
}

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

    let tray_icon = Image::from_bytes(include_bytes!("../icons/tray-44x44.png"))
        .expect("failed to load tray icon");

    let _tray = TrayIconBuilder::with_id("main")
        .tooltip("Sery Link — starting…")
        .icon(tray_icon)
        .icon_as_template(true)
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

/// Rebuild the tray menu with current state + stats.
///
/// Dispatches the actual AppKit work (NSStatusItem mutation via
/// `set_menu` / `set_tooltip`) to the main thread. Calling those from a
/// background tokio worker — which is what happens when the watcher's
/// sync_folder flips the tray to "syncing" — throws an Obj-C exception
/// on macOS that unwinds through Rust and aborts the process. The
/// `app.run_on_main_thread` hop keeps the operation on the one thread
/// AppKit is happy with.
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let app_clone = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = TRAY_STATE.read().map(|s| s.clone()).unwrap_or_default();
        if let Some(tray) = app_clone.tray_by_id("main") {
            if let Ok(menu) = build_menu(&app_clone, &state) {
                let _ = tray.set_menu(Some(menu));
            }
            let _ = tray.set_tooltip(Some(tooltip_for(&state)));

            // Swap icon: badge states use a non-template icon with a colored
            // dot in the bottom-right corner; offline falls back to the plain
            // template icon so macOS auto-adapts it for dark/light mode.
            match badge_icon_bytes(&state.connection) {
                Some(bytes) => {
                    if let Ok(icon) = Image::from_bytes(&bytes) {
                        let _ = tray.set_icon(Some(icon));
                        let _ = tray.set_icon_as_template(false);
                    }
                }
                None => {
                    if let Ok(icon) = Image::from_bytes(BASE_ICON) {
                        let _ = tray.set_icon(Some(icon));
                        let _ = tray.set_icon_as_template(true);
                    }
                }
            }
        }
    });
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

    let open_web = MenuItem::with_id(app, MI_OPEN_WEB, "Open Dashboard", true, None::<&str>)?;

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
