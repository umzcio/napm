mod http;
mod scan;
mod store;
mod ops;
mod search;

use scan::InstalledTool;
use store::{HistoryEntry, Store};
use tauri::Manager;

/// Open the Store rooted at the platform app-data directory.
fn open_store(app: &tauri::AppHandle) -> Store {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    Store::new(dir)
}

#[tauri::command]
fn scan_installed(app: tauri::AppHandle) -> Vec<InstalledTool> {
    let pins = open_store(&app).pins();
    scan::scan_all(&pins)
}

#[tauri::command]
fn set_pin(app: tauri::AppHandle, pkg: String, pinned: bool) {
    open_store(&app).set_pin(&pkg, pinned);
}

#[tauri::command]
fn get_history(app: tauri::AppHandle) -> Vec<HistoryEntry> {
    open_store(&app).history()
}

#[tauri::command]
fn run_op(
    app: tauri::AppHandle,
    op_id: String,
    eco: String,
    pkg: String,
    from: Option<String>,
    to: String,
    action: String,
) {
    let store = open_store(&app);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    ops::run_op(app.clone(), store, op_id, eco, pkg, from, to, action, ts);
}

#[tauri::command]
fn search_registry(app: tauri::AppHandle, query: String) -> Vec<search::SearchResult> {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    search::search_all(&query, &dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![scan_installed, set_pin, get_history, run_op, search_registry])
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      // Warm the brew catalog in the background so the first search is not cold.
      // Best-effort: never block startup on the network.
      let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
      std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        search::brew::warm_brew(&dir);
      });
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
