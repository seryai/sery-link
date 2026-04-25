// ROADMAP F3 (and partial F1) — `seryai://` URL-scheme handler.
//
// Registered via tauri-plugin-deep-link + the `plugins.deep-link.desktop.schemes`
// entry in tauri.conf.json. When the OS hands the running Sery Link
// process an `seryai://...` URL, this module parses it and dispatches.
//
// Verbs supported in v0.5.0:
//   - `seryai://reveal?path=<absolute-path>`
//       Opens the file's containing folder in Finder/Explorer with the
//       file selected. Closes the F3 click-to-detail "you can copy a
//       path but can't open it" gap — the dashboard's search results
//       can now build this URL and let the OS route to Sery Link.
//
// Deferred (v0.5.x):
//   - `seryai://pair?key=<workspace-key>` — would close the F1 deep-link
//     pairing alternative (a clickable invite from email/chat). The QR
//     codes already encode this URI shape; the handler scaffold below
//     accepts the verb but currently no-ops, returning an early-decline
//     so the user understands the verb is recognised but not finished.

use tauri::{AppHandle, Manager, Runtime};
use url::Url;

/// Dispatch a single `seryai://` URL to the appropriate handler.
/// Called from the deep-link plugin's on_open_url callback in lib.rs.
/// Failures are logged but never crash the app — a malformed URL
/// shouldn't take the process down.
pub fn handle_url<R: Runtime>(app: &AppHandle<R>, raw_url: &str) {
    let url = match Url::parse(raw_url) {
        Ok(u) => u,
        Err(err) => {
            eprintln!("[deep-link] could not parse {raw_url}: {err}");
            return;
        }
    };

    if url.scheme() != "seryai" {
        eprintln!("[deep-link] unexpected scheme: {}", url.scheme());
        return;
    }

    // Surface the main window — the user clicked a link expecting Sery
    // to come to the front, regardless of which verb fires.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    #[cfg(target_os = "macos")]
    {
        use tauri::ActivationPolicy;
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
    }

    // The verb lives in the host portion (e.g. `seryai://reveal?path=...`)
    // because url::Url treats anything after `://` as host until `/`.
    let verb = url.host_str().unwrap_or("").to_lowercase();

    match verb.as_str() {
        "reveal" => handle_reveal(&url),
        "pair" => handle_pair(app, &url),
        other => {
            eprintln!("[deep-link] unknown verb: {other}");
        }
    }
}

fn handle_reveal(url: &Url) {
    let path = url
        .query_pairs()
        .find(|(k, _)| k == "path")
        .map(|(_, v)| v.into_owned());

    let path = match path {
        Some(p) if !p.is_empty() => p,
        _ => {
            eprintln!("[deep-link] reveal: missing or empty `path` parameter");
            return;
        }
    };

    let p = std::path::PathBuf::from(&path);
    // Mirror the existing reveal_in_finder Tauri command behaviour:
    // open the parent directory if the path is a file (so the OS file
    // manager highlights it); open the path itself if it's a directory.
    let target = if p.is_file() {
        p.parent().map(|x| x.to_path_buf()).unwrap_or(p)
    } else {
        p
    };

    if let Err(err) = open::that(target) {
        eprintln!("[deep-link] reveal: failed to open path {path}: {err}");
    }
}

fn handle_pair<R: Runtime>(app: &AppHandle<R>, url: &Url) {
    // First-cut placeholder for the v0.5.x deep-link pairing alternative.
    // Surface the main window (already done by the dispatcher) and emit
    // an event the frontend can listen to once the join-existing-workspace
    // UI lands. Today this just logs the key so a developer poking at
    // the URL scheme sees it works end-to-end on the routing side.
    let key = url
        .query_pairs()
        .find(|(k, _)| k == "key")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();

    if key.is_empty() {
        eprintln!("[deep-link] pair: missing or empty `key` parameter");
        return;
    }

    eprintln!(
        "[deep-link] pair: received workspace key (len={}) — UI handler not wired yet",
        key.len()
    );

    // Emit a frontend event so a future Onboarding/Connect handler can
    // pre-fill the key field and prompt the user to confirm. The
    // payload is the raw key — the receiving component is responsible
    // for confirming with the user before calling auth_with_key.
    use tauri::Emitter;
    if let Err(err) = app.emit("deep-link-pair", &key) {
        eprintln!("[deep-link] pair: failed to emit event: {err}");
    }
}
