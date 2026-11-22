//! Native host for the shared Spellwake frontend.
#![cfg_attr(not(mobile), forbid(unsafe_code))]

/// Start the native application with only explicitly granted frontend capabilities.
///
/// # Errors
/// Returns the native host error if initialization or the event loop fails.
pub fn run() -> tauri::Result<()> {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        use tauri::Manager;
        if let Some(window) = app.get_webview_window("main")
            && window
                .unminimize()
                .and_then(|()| window.show())
                .and_then(|()| window.set_focus())
                .is_err()
        {
            eprintln!("native_auth code=focus_unavailable");
        }
    }));
    builder
        .plugin(tauri_plugin_deep_link::init())
        // Saving a file the player asked for: the system dialog picks the place, and the
        // file plugin may then write to that one place. The capability grants nothing wider.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .run(tauri::generate_context!())
}

/// Enter through Tauri's generated mobile platform bindings.
#[cfg(mobile)]
#[tauri::mobile_entry_point]
fn mobile_main() {
    if let Err(error) = run() {
        eprintln!("Spellwake could not start: {error}");
    }
}
