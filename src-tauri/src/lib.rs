//! Native host for the shared Spellwake frontend.
#![cfg_attr(not(mobile), forbid(unsafe_code))]

/// Start the native application with only explicitly granted frontend capabilities.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default().run(tauri::generate_context!())
}

/// Enter through Tauri's generated mobile platform bindings.
#[cfg(mobile)]
#[tauri::mobile_entry_point]
fn mobile_main() {
    if let Err(error) = run() {
        eprintln!("Spellwake could not start: {error}");
    }
}
