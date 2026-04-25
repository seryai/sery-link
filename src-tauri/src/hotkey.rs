//! ROADMAP F9 — Quick-Ask global hotkey.
//!
//! Registers an OS-level shortcut (⌘⇧S on macOS, Ctrl+Shift+S on
//! Windows/Linux) that, when pressed from anywhere, shows + focuses
//! the Sery Link main window and navigates to the search/ask surface
//! with the input focused.
//!
//! **First cut scope** — uses the existing main window. A dedicated
//! floating overlay (Spotlight-style separate Tauri window) is the
//! v0.6+ shape; for v0.5.0 the natural surface is the search page
//! that the app already redirects to from `/`.
//!
//! Design constraints from VISION.md §10:
//!   1. Strengthens the network — the hotkey is the surface that
//!      makes Sery feel always-available across all your machines,
//!      not just one.
//!   2. Doesn't require Sery to see anything it shouldn't — pure
//!      local OS event; nothing leaves the machine.
//!   3. Each new endpoint compounds — a hotkey on machine A is
//!      identical to a hotkey on machine B; the network "feels the
//!      same" everywhere.
//!   4. Disconnected-OSS path stays supported — the hotkey works
//!      even when the network is offline (it still shows the local
//!      search surface).

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};

/// Public name of the shortcut, surfaced in tooltips and Settings.
pub const SHORTCUT_LABEL: &str = if cfg!(target_os = "macos") {
    "⌘⇧S"
} else {
    "Ctrl+Shift+S"
};

/// Build the canonical Quick-Ask shortcut for the current platform.
///
/// macOS: `Cmd+Shift+S` (matches the Spotlight pattern of using ⌘ as
/// the primary modifier).
/// Windows/Linux: `Ctrl+Shift+S` (the de-facto cross-platform analogue).
fn quick_ask_shortcut() -> Shortcut {
    #[cfg(target_os = "macos")]
    let modifiers = Modifiers::SUPER | Modifiers::SHIFT;
    #[cfg(not(target_os = "macos"))]
    let modifiers = Modifiers::CONTROL | Modifiers::SHIFT;

    Shortcut::new(Some(modifiers), Code::KeyS)
}

/// Register the Quick-Ask hotkey on the given app handle. Idempotent
/// for app-lifetime calls (the plugin's internal registry handles
/// duplicates), but should be called once at setup.
///
/// On hotkey press:
///   1. The main window is shown and focused (creating a "summon" feel
///      whether the app was minimized, hidden in the tray, or just out
///      of focus on another desktop).
///   2. A `quick-ask` event is emitted to the frontend. The SearchPage
///      listens for it and (a) navigates to `/search` if not already
///      there, (b) focuses the search input, (c) clears any previous
///      query so the user can type immediately.
///
/// Failures are logged but non-fatal: a missing hotkey shouldn't crash
/// the app, just degrade the UX.
pub fn register<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let shortcut = quick_ask_shortcut();
    let shortcut_for_handler = shortcut.clone();

    let plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, triggered, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if triggered != &shortcut_for_handler {
                return;
            }
            on_quick_ask(app);
        })
        .build();

    app.plugin(plugin)?;

    // The plugin must be installed before `register` is callable on the
    // GlobalShortcut runtime; the Builder above only declared the
    // handler. Now register the actual binding. The plugin's error
    // type doesn't auto-convert into tauri::Error so failures are
    // logged-and-continued rather than propagated — a missing hotkey
    // shouldn't crash the app.
    if let Err(err) = app.global_shortcut().register(shortcut) {
        eprintln!(
            "[hotkey] could not bind {SHORTCUT_LABEL} as a global shortcut: {err}. \
             Quick-Ask will not be available; everything else still works."
        );
    }

    Ok(())
}

/// Surface the main window + ask the frontend to focus the search
/// input. Called from the global-shortcut handler.
fn on_quick_ask<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        // Order matters: unminimize → show → focus. On some platforms
        // (macOS in particular) calling set_focus on a hidden window
        // is a no-op, so show first.
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        // No main window? Nothing to focus — the hotkey is a no-op.
        // This only happens during early boot or after the user
        // explicitly closed the window in a way that destroyed it.
        return;
    }

    // Emit the event that the SearchPage listens for. Payload is the
    // shortcut label so the frontend can show a one-time "summoned via
    // ⌘⇧S" hint on first use if it wants to.
    if let Err(err) = app.emit("quick-ask", SHORTCUT_LABEL) {
        eprintln!("[hotkey] failed to emit quick-ask event: {err}");
    }

    // macOS-specific: ensure the dock icon comes back if the window was
    // hidden (we set Accessory policy when the window is hidden in
    // lib.rs's on_window_event handler).
    #[cfg(target_os = "macos")]
    {
        use tauri::ActivationPolicy;
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
    }
}
