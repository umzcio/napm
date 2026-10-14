mod cache;
mod http;
mod importer;
mod intel;
mod ops;
mod pathenv;
mod scan;
mod search;
mod store;

use scan::InstalledTool;
use store::{HistoryEntry, Store};
use tauri::Manager;

/// The platform app-data directory, or the working directory if the platform
/// cannot resolve one. Not created here; callers that write create it.
fn app_data_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Open the Store rooted at the platform app-data directory.
fn open_store(app: &tauri::AppHandle) -> Store {
    Store::new(app_data_dir(app))
}

/// Current Unix time in whole seconds, or 0 if the clock reads before 1970.
pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Lock `m`, recovering the guard if a panicking thread poisoned it. Every
/// mutex in napm guards either `()` or cache data whose updates are single
/// insert/remove/extend/assign calls, so a panic cannot leave it half-written.
/// Propagating the poison instead would turn one panicked command into a
/// panic on every later call that touches the same lock.
pub(crate) fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Run blocking work (shell-outs, `ureq` HTTP, filesystem walks) on the async
/// runtime's blocking pool. A sync `#[tauri::command(async)]` body runs
/// directly on a runtime worker thread, so a slow `brew` call or a network
/// timeout would occupy that worker for its whole duration and could starve
/// the genuinely async commands (the updater). A panic in `f` comes back as an
/// `Err` that rejects the frontend promise instead of leaving it pending.
async fn blocking<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn scan_installed(app: tauri::AppHandle) -> Result<Vec<InstalledTool>, String> {
    blocking(move || {
        let store = open_store(&app);
        let pins = store.pins();
        let settings = store.settings();
        scan::scan_all(&pins, settings.sources, settings.probe_manual, store.dir())
    })
    .await
}

#[tauri::command(async)]
fn set_pin(app: tauri::AppHandle, pkg: String, pinned: bool) -> Result<(), String> {
    open_store(&app)
        .set_pin(&pkg, pinned)
        .map_err(|e| e.to_string())
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
    let ts = unix_now();
    ops::run_op(app, store, op_id, eco, pkg, from, to, action, ts);
}

/// Installed Homebrew formulae that currently depend on `pkg`, used by the
/// uninstall confirm modal to warn (and disable) before running a real
/// `brew uninstall`. Best effort: an empty result means either there truly
/// are no dependents, or the check itself could not run; either way the
/// caller does not block on it, since the real `brew uninstall` remains the
/// source of truth and streams its own honest failure.
#[tauri::command]
async fn brew_uninstall_check(pkg: String) -> Result<Vec<String>, String> {
    blocking(move || ops::brew_dependents(&pkg)).await
}

#[tauri::command]
async fn search_registry(
    app: tauri::AppHandle,
    query: String,
) -> Result<Vec<search::SearchResult>, String> {
    blocking(move || {
        let dir = app_data_dir(&app);
        let _ = std::fs::create_dir_all(&dir);
        let sources = open_store(&app).settings().sources;
        search::search_all(&query, &dir, sources)
    })
    .await
}

#[tauri::command]
async fn get_whats_new(
    app: tauri::AppHandle,
    installed: Vec<intel::ToolRef>,
    verdict_scope: Vec<intel::ScopeRef>,
) -> Result<intel::WhatsNew, String> {
    blocking(move || {
        let dir = app_data_dir(&app);
        let _ = std::fs::create_dir_all(&dir);
        let now = unix_now();
        let advisories_enabled = open_store(&app).settings().advisory_checks;
        intel::whats_new(&installed, &verdict_scope, &dir, now, advisories_enabled)
    })
    .await
}

#[tauri::command]
async fn get_changelog(
    app: tauri::AppHandle,
    eco: String,
    pkg: String,
    version: String,
) -> Result<Vec<String>, String> {
    blocking(move || {
        let dir = app_data_dir(&app);
        let _ = std::fs::create_dir_all(&dir);
        intel::release::changelog(&eco, &pkg, &version, &dir)
    })
    .await
}

#[tauri::command]
async fn get_advisory(id: String) -> Result<Option<intel::Advisory>, String> {
    blocking(move || {
        intel::osv::fetch_advisory(&id).map(|(severity, summary, fixed_version)| intel::Advisory {
            severity,
            summary,
            fixed_version,
        })
    })
    .await
}

#[tauri::command(async)]
fn get_settings(app: tauri::AppHandle) -> store::Settings {
    open_store(&app).settings()
}

#[tauri::command(async)]
fn set_settings(app: tauri::AppHandle, settings: store::Settings) -> Result<(), String> {
    open_store(&app)
        .set_settings(&settings)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn export_library(
    app: tauri::AppHandle,
    filename: String,
    content: String,
) -> Result<String, String> {
    let dir = app_data_dir(&app);
    let _ = std::fs::create_dir_all(&dir);
    // Sanitize the frontend-supplied filename: no path separators or traversal.
    let safe = cache::sanitize_key(&filename);
    let path = dir.join(safe);
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    // -R reveals and selects the new file in Finder. Best-effort: the export
    // itself already succeeded, and the status line reports its path.
    let _ = macos_open([std::ffi::OsStr::new("-R"), path.as_os_str()]);
    Ok(path.to_string_lossy().to_string())
}

/// Parse+validate an import manifest, then bucket its rows against a fresh
/// scan: already-installed (eco-aware match), will-install (not present, not
/// pinned to the manifest's old version -- see importer's module doc), or
/// cannot-install (manual/npx/unknown eco, or a malformed row) with a reason.
/// Best-effort network resolution upgrades each will-install row's version to
/// the ecosystem's current latest; a failed lookup keeps the manifest's
/// recorded version rather than blocking the whole import.
#[tauri::command]
async fn import_preview(
    app: tauri::AppHandle,
    manifest_json: String,
) -> Result<importer::ImportPreview, String> {
    blocking(move || {
        let manifest = importer::parse_manifest(&manifest_json)?;
        let store = open_store(&app);
        let pins = store.pins();
        let settings = store.settings();
        let installed = scan::scan_all(&pins, settings.sources, settings.probe_manual, store.dir());
        let mut preview = importer::classify(&manifest, &installed);
        importer::resolve_latest_for_will_install(&mut preview, store.dir());
        Ok(preview)
    })
    .await?
}

/// Run macOS `open` with `args` and wait for it. `open` hands the request to
/// Launch Services and exits at once, so waiting is cheap; it also reaps the
/// child (a bare `spawn()` left a zombie process per call) and turns a failure
/// into an error the frontend's status line shows, instead of a silent no-op.
fn macos_open<I, S>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let out = std::process::Command::new("open")
        .args(args)
        .output()
        .map_err(|e| format!("could not run open: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[tauri::command(async)]
fn open_data_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = app_data_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    macos_open([&dir])
}

#[tauri::command(async)]
fn open_external(url: String) -> Result<(), String> {
    // Only open secure web URLs, never arbitrary local paths or args.
    if !url.starts_with("https://") {
        return Err("only https:// links can be opened".to_string());
    }
    macos_open([&url])
}

#[derive(serde::Serialize)]
struct NpxLatest {
    pkg: String,
    latest: String,
}

/// For each npx package name, resolve the registry `dist-tags.latest` (the
/// version npx fetches on the next `@latest` run). Best-effort: unresolved
/// packages are omitted. Used by the frontend to show a drift hint on npx rows.
/// Goes through the shared registry-document cache and is bounded at 8
/// concurrent workers rather than one thread per package.
#[tauri::command]
async fn npx_latest(app: tauri::AppHandle, pkgs: Vec<String>) -> Result<Vec<NpxLatest>, String> {
    blocking(move || {
        let dir = app_data_dir(&app);
        let _ = std::fs::create_dir_all(&dir);

        let mut results: Vec<Option<NpxLatest>> = (0..pkgs.len()).map(|_| None).collect();
        std::thread::scope(|inner| {
            let handles: Vec<_> = intel::registry::chunk_indices(pkgs.len(), 8)
                .into_iter()
                .map(|idxs| {
                    let pkgs = &pkgs;
                    let dir = &dir;
                    inner.spawn(move || -> Vec<(usize, NpxLatest)> {
                        idxs.into_iter()
                            .filter_map(|i| {
                                let pkg = pkgs[i].clone();
                                let body = intel::registry::doc("npm", &pkg, dir)?;
                                let latest = scan::npx::parse_dist_tag_latest(&body)?;
                                Some((i, NpxLatest { pkg, latest }))
                            })
                            .collect()
                    })
                })
                .collect();
            for h in handles {
                if let Ok(pairs) = h.join() {
                    for (i, info) in pairs {
                        results[i] = Some(info);
                    }
                }
            }
        });
        results.into_iter().flatten().collect()
    })
    .await
}

/// Reveal and select a path in Finder (`open -R`). Validates the path exists so
/// a stale entry never shells an arbitrary string; a missing path is an error
/// the status line shows rather than a click that silently does nothing.
#[tauri::command(async)]
fn reveal_in_finder(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("{path} no longer exists"));
    }
    macos_open(["-R", path.as_str()])
}

#[tauri::command(async)]
fn clear_caches(app: tauri::AppHandle) {
    let dir = app_data_dir(&app);
    for name in [
        "brew_catalog.json",
        "brew_analytics.json",
        "wire.json",
        "manual_probe.json",
    ] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    // Drop the in-memory parsed catalog too, else the fresh in-memory copy masks
    // the deletion and the re-warm below just returns the stale data.
    search::brew::invalidate_catalog();
    // Same for the in-memory registry-document cache.
    intel::registry::invalidate();
    // Remove the per-version changelog/hold caches and the registry-document
    // cache (changelog_*.json, hold_*.json, regdoc_*.json).
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if (n.starts_with("changelog_") || n.starts_with("hold_") || n.starts_with("regdoc_"))
                && n.ends_with(".json")
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    // Re-warm the brew catalog in the background so the next search is not cold.
    std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        search::brew::warm_brew(&dir);
    });
}

/// Minimal update metadata sent to the frontend. Empty strings where unknown.
#[derive(serde::Serialize)]
struct UpdateMeta {
    version: String,
    notes: String,
    #[serde(rename = "pubDate")]
    pub_date: String,
}

/// Check the release feed for a newer signed version.
///
/// `Ok(None)` means the check ran and found no update: the app is up to
/// date. `Ok(Some(meta))` means an update is available. `Err(msg)` means the
/// check itself could not run (updater init failure, network error, a
/// malformed manifest, etc). These are distinct: a failure is not the same
/// claim as "you are up to date," and the frontend must not conflate them.
#[tauri::command(async)]
async fn check_for_update(app: tauri::AppHandle) -> Result<Option<UpdateMeta>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| UpdateMeta {
        version: u.version,
        notes: u.body.unwrap_or_default(),
        pub_date: u.date.map(|d| d.to_string()).unwrap_or_default(),
    }))
}

/// Download, verify (against the baked-in pubkey), install, and relaunch.
/// Returns an honest error string on any failure; never a silent partial install.
#[tauri::command(async)]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no update available".to_string())?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Capture the real login-shell PATH before anything spawns, so a Dock/Finder
    // launch can find npm/brew/pip and the manual scanner can walk a real $PATH.
    pathenv::fix_path();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            scan_installed,
            set_pin,
            get_history,
            run_op,
            brew_uninstall_check,
            search_registry,
            get_whats_new,
            get_changelog,
            get_advisory,
            open_data_dir,
            open_external,
            clear_caches,
            get_settings,
            set_settings,
            export_library,
            import_preview,
            reveal_in_finder,
            check_for_update,
            install_update,
            npx_latest
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            // Warm the brew catalog in the background so the first search is not cold.
            // Best-effort: never block startup on the network.
            let dir = app_data_dir(app.handle());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn lock_should_recover_guard_after_a_panicking_holder_poisons_the_mutex() {
        let m = Mutex::new(vec![1]);
        let _ = std::thread::scope(|s| {
            s.spawn(|| {
                let _g = m.lock().unwrap();
                panic!("poison the lock");
            })
            .join()
        });
        assert!(m.is_poisoned());
        lock(&m).push(2);
        assert_eq!(*lock(&m), vec![1, 2]);
    }

    #[test]
    fn open_external_should_reject_non_https_urls_without_running_open() {
        let err = open_external("file:///etc/passwd".to_string()).unwrap_err();
        assert_eq!(err, "only https:// links can be opened");
    }

    #[test]
    fn reveal_in_finder_should_report_a_missing_path_instead_of_doing_nothing() {
        let err = reveal_in_finder("/nonexistent/napm-test-path".to_string()).unwrap_err();
        assert_eq!(err, "/nonexistent/napm-test-path no longer exists");
    }
}
