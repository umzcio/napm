mod http;
mod scan;
mod store;
mod ops;
mod intel;
mod search;
mod pathenv;

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

#[tauri::command(async)]
fn scan_installed(app: tauri::AppHandle) -> Vec<InstalledTool> {
    let store = open_store(&app);
    let pins = store.pins();
    scan::scan_all(&pins, store.settings().sources)
}

#[tauri::command(async)]
fn set_pin(app: tauri::AppHandle, pkg: String, pinned: bool) {
    open_store(&app).set_pin(&pkg, pinned);
}

#[tauri::command(async)]
fn get_history(app: tauri::AppHandle) -> Vec<HistoryEntry> {
    open_store(&app).history()
}

#[tauri::command(async)]
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

#[tauri::command(async)]
fn search_registry(app: tauri::AppHandle, query: String) -> Vec<search::SearchResult> {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    let sources = open_store(&app).settings().sources;
    search::search_all(&query, &dir, sources)
}

#[tauri::command(async)]
fn get_whats_new(app: tauri::AppHandle, installed: Vec<intel::ToolRef>, verdict_scope: Vec<String>) -> intel::WhatsNew {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    intel::whats_new(&installed, &verdict_scope, &dir, now)
}

#[tauri::command(async)]
fn get_changelog(app: tauri::AppHandle, eco: String, pkg: String, version: String) -> Vec<String> {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    intel::release::changelog(&eco, &pkg, &version, &dir)
}

#[tauri::command(async)]
fn get_advisory(id: String) -> Option<intel::Advisory> {
    intel::osv::fetch_advisory(&id).map(|(severity, summary, fixed_version)| intel::Advisory {
        severity,
        summary,
        fixed_version,
    })
}

#[tauri::command(async)]
fn get_settings(app: tauri::AppHandle) -> store::Settings {
    open_store(&app).settings()
}

#[tauri::command(async)]
fn set_settings(app: tauri::AppHandle, settings: store::Settings) {
    open_store(&app).set_settings(&settings);
}

#[tauri::command(async)]
fn export_library(app: tauri::AppHandle, filename: String, content: String) {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    // Sanitize the frontend-supplied filename: no path separators or traversal.
    let safe = filename.replace(['/', '\\'], "_").replace("..", "_");
    let path = dir.join(safe);
    if std::fs::write(&path, content).is_ok() {
        // -R reveals and selects the new file in Finder.
        let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
    }
}

#[tauri::command(async)]
fn open_data_dir(app: tauri::AppHandle) {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::process::Command::new("open").arg(&dir).spawn();
}

#[tauri::command(async)]
fn open_external(url: String) {
    // Only open secure web URLs, never arbitrary local paths or args.
    if url.starts_with("https://") {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    }
}

/// Reveal and select a path in Finder (`open -R`). Validates the path exists so
/// a stale entry never shells an arbitrary string. No-op on a missing path.
#[tauri::command(async)]
fn reveal_in_finder(path: String) {
    let p = std::path::Path::new(&path);
    if p.exists() {
        let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
    }
}

#[tauri::command(async)]
fn clear_caches(app: tauri::AppHandle) {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    for name in ["brew_catalog.json", "brew_analytics.json", "wire.json"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    // Drop the in-memory parsed catalog too, else the fresh in-memory copy masks
    // the deletion and the re-warm below just returns the stale data.
    search::brew::invalidate_catalog();
    // Remove the per-version changelog caches (changelog_*.json).
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if n.starts_with("changelog_") && n.ends_with(".json") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    // Re-warm the brew catalog in the background so the next search is not cold.
    std::thread::spawn(move || {
        let d = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let _ = std::fs::create_dir_all(&d);
        search::brew::warm_brew(&d);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  // Capture the real login-shell PATH before anything spawns, so a Dock/Finder
  // launch can find npm/brew/pip and the manual scanner can walk a real $PATH.
  pathenv::fix_path();
  tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![scan_installed, set_pin, get_history, run_op, search_registry, get_whats_new, get_changelog, get_advisory, open_data_dir, open_external, clear_caches, get_settings, set_settings, export_library, reveal_in_finder])
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
      // One-time migration from the pre-rename app-data dir (com.tauri.dev).
      if let Some(parent) = dir.parent() {
        store::migrate_legacy(&dir, &parent.join("com.tauri.dev"));
      }
      std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        search::brew::warm_brew(&dir);
      });
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
