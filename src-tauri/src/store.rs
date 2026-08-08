use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Process-wide lock guarding read-modify-write sequences on pins/history/settings.
/// `Store` is constructed fresh per command call, so this must be a `static`
/// rather than a per-instance lock to actually serialize concurrent callers.
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// One logged version change. Mirrors the prototype's HistoryEntry, plus `eco`
/// so rollback can rebuild the command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub ts: i64,
    pub pkg: String,
    pub eco: String,
    pub action: String, // "install" | "update" | "rollback"
    pub from: Option<String>,
    pub to: String,
}

/// Which ecosystems the scan and search cover. Defaults to all enabled.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sources {
    pub npm: bool,
    pub brew: bool,
    pub pip: bool,
    pub npx: bool,
    pub manual: bool,
}
impl Default for Sources {
    fn default() -> Self {
        Sources {
            npm: true,
            brew: true,
            pip: true,
            npx: true,
            manual: true,
        }
    }
}

/// Persisted user settings. Missing or corrupt file reads as defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub github_token: String,
    pub sources: Sources,
    /// Whether the manual scanner may run `<tool> --version`/`version` on
    /// $HOME binaries whose filename carries no version token. Defaults to
    /// true (existing behavior); an old settings.json missing this key still
    /// deserializes to true via `#[serde(default)]` reading the field off
    /// `Settings::default()` below.
    pub probe_manual: bool,
    /// Whether the What's New feed may send the installed-package inventory
    /// (names and versions) to OSV.dev for the advisory scan. Defaults to
    /// true (existing behavior); an old settings.json missing this key still
    /// deserializes to true via `#[serde(default)]` reading the field off
    /// `Settings::default()` below. When false, `intel::whats_new` must skip
    /// the OSV call entirely and report a distinct "disabled" state rather
    /// than a clean result.
    pub advisory_checks: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            github_token: String::new(),
            sources: Sources::default(),
            probe_manual: true,
            advisory_checks: true,
        }
    }
}

/// Flat-JSON persistence for pins and history in a single directory.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        let _ = std::fs::create_dir_all(&dir);
        // Best-effort: tighten the mode on a pre-existing settings.json so
        // upgrading users get the owner-only fix without re-saving.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let settings_path = dir.join("settings.json");
            if settings_path.exists() {
                let _ = std::fs::set_permissions(
                    &settings_path,
                    std::fs::Permissions::from_mode(0o600),
                );
            }
        }
        Store { dir }
    }

    fn pins_path(&self) -> PathBuf {
        self.dir.join("pins.json")
    }
    fn history_path(&self) -> PathBuf {
        self.dir.join("history.json")
    }

    fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Writes via a sibling temp file plus rename, which is atomic on the same
    /// filesystem: readers always see a complete old or new file, never a
    /// partial write from a crash mid-write.
    fn write_json<T: Serialize>(path: &Path, value: &T) {
        if let Ok(s) = serde_json::to_string_pretty(value) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
                }
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    pub fn pins(&self) -> BTreeSet<String> {
        Self::read_json(&self.pins_path())
    }

    pub fn set_pin(&self, pkg: &str, on: bool) {
        let _g = STORE_LOCK.lock().unwrap();
        let mut pins = self.pins();
        if on {
            pins.insert(pkg.to_string());
        } else {
            pins.remove(pkg);
        }
        Self::write_json(&self.pins_path(), &pins);
    }

    /// History newest-first.
    pub fn history(&self) -> Vec<HistoryEntry> {
        let mut h: Vec<HistoryEntry> = Self::read_json(&self.history_path());
        h.sort_by_key(|e| std::cmp::Reverse(e.ts));
        h
    }

    pub fn add_history(&self, entry: HistoryEntry) {
        let _g = STORE_LOCK.lock().unwrap();
        let mut h: Vec<HistoryEntry> = Self::read_json(&self.history_path());
        h.push(entry);
        Self::write_json(&self.history_path(), &h);
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    pub fn settings(&self) -> Settings {
        Self::read_json(&self.settings_path())
    }

    pub fn set_settings(&self, s: &Settings) {
        let _g = STORE_LOCK.lock().unwrap();
        Self::write_json(&self.settings_path(), s);
    }

    /// The app-data directory this store is rooted at. Used by callers (e.g.
    /// the manual scanner's probe cache) that need a cache file alongside
    /// pins/history/settings.json but are not part of Store's own state.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    #[cfg(test)]
    pub fn dir_for_test(&self) -> PathBuf {
        self.dir.clone()
    }
}

/// One-time, best-effort migration of user-data files from a legacy app-data
/// directory into the current one. Only `pins/history/settings.json` are copied,
/// and only when the target does not already exist (never clobber newer data).
/// Caches are intentionally skipped (they regenerate). No-op if the legacy dir
/// is absent. Any IO error is ignored so this never blocks startup.
pub fn migrate_legacy(current_dir: &Path, legacy_dir: &Path) {
    if !legacy_dir.is_dir() {
        return;
    }
    let _ = std::fs::create_dir_all(current_dir);
    for f in ["pins.json", "history.json", "settings.json"] {
        let dst = current_dir.join(f);
        let src = legacy_dir.join(f);
        if !dst.exists() && src.exists() {
            let _ = std::fs::copy(&src, &dst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> Store {
        // unique dir per test process+address; no Date/rand needed
        let mut dir = std::env::temp_dir();
        dir.push(format!("napm-test-{:p}", &dir));
        let _ = std::fs::remove_dir_all(&dir);
        Store::new(dir)
    }

    #[test]
    fn pins_round_trip_and_dedupe() {
        let s = temp_store();
        assert!(s.pins().is_empty());
        s.set_pin("typescript", true);
        s.set_pin("typescript", true); // idempotent
        s.set_pin("eslint", true);
        let pins = s.pins();
        assert!(pins.contains("typescript") && pins.contains("eslint") && pins.len() == 2);
        s.set_pin("typescript", false);
        assert!(!s.pins().contains("typescript"));
    }

    #[test]
    fn history_appends_newest_first() {
        let s = temp_store();
        assert!(s.history().is_empty());
        s.add_history(HistoryEntry {
            ts: 1,
            pkg: "a".into(),
            eco: "npm".into(),
            action: "install".into(),
            from: None,
            to: "1.0".into(),
        });
        s.add_history(HistoryEntry {
            ts: 2,
            pkg: "b".into(),
            eco: "npm".into(),
            action: "update".into(),
            from: Some("1.0".into()),
            to: "2.0".into(),
        });
        let h = s.history();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].pkg, "b"); // newest first
        assert_eq!(h[1].pkg, "a");
    }

    #[test]
    fn missing_or_corrupt_files_read_as_empty() {
        let s = temp_store();
        std::fs::create_dir_all(s_dir(&s)).unwrap();
        std::fs::write(s_dir(&s).join("pins.json"), b"not json").unwrap();
        assert!(s.pins().is_empty()); // corrupt -> empty, no panic
                                      // Later plans may want to surface this to the user instead of silently
                                      // treating it as empty; for now the fallback behavior is unchanged.
    }

    #[test]
    fn add_history_roundtrip_leaves_no_tmp_file() {
        let s = temp_store();
        s.add_history(HistoryEntry {
            ts: 1,
            pkg: "a".into(),
            eco: "npm".into(),
            action: "install".into(),
            from: None,
            to: "1.0".into(),
        });
        let h = s.history();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].pkg, "a");
        assert!(!s.dir_for_test().join("history.json.tmp").exists());
    }

    #[test]
    fn concurrent_add_history_does_not_lose_entries() {
        use std::sync::Arc;
        let s = Arc::new(temp_store());
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let s = Arc::clone(&s);
                std::thread::spawn(move || {
                    s.add_history(HistoryEntry {
                        ts: i,
                        pkg: format!("pkg{}", i),
                        eco: "npm".into(),
                        action: "install".into(),
                        from: None,
                        to: "1.0".into(),
                    });
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(s.history().len(), 8);
    }

    fn s_dir(s: &Store) -> PathBuf {
        s.dir_for_test()
    }

    #[test]
    fn settings_round_trip() {
        let s = temp_store();
        let def = s.settings();
        assert_eq!(def.github_token, "");
        assert!(def.sources.npm && def.sources.brew && def.sources.pip && def.sources.npx);
        assert!(def.probe_manual);
        assert!(def.advisory_checks);
        s.set_settings(&Settings {
            github_token: "abc".into(),
            sources: Sources {
                npm: true,
                brew: false,
                pip: true,
                npx: true,
                manual: true,
            },
            probe_manual: false,
            advisory_checks: false,
        });
        let got = s.settings();
        assert_eq!(got.github_token, "abc");
        assert!(!got.sources.brew);
        assert!(got.sources.npm && got.sources.pip);
        assert!(!got.probe_manual);
        assert!(!got.advisory_checks);
    }

    #[test]
    #[cfg(unix)]
    fn settings_file_is_owner_only_mode() {
        use std::os::unix::fs::PermissionsExt;
        let s = temp_store();
        s.set_settings(&Settings {
            github_token: "secret-token".into(),
            sources: Sources::default(),
            probe_manual: true,
            advisory_checks: true,
        });
        let perm = std::fs::metadata(s.dir_for_test().join("settings.json"))
            .unwrap()
            .permissions();
        assert_eq!(perm.mode() & 0o777, 0o600);
    }

    #[test]
    #[cfg(unix)]
    fn preexisting_settings_file_is_remoded_on_store_new() {
        use std::os::unix::fs::PermissionsExt;
        let mut dir = std::env::temp_dir();
        dir.push(format!("napm-test-remode-{:p}", &dir));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), b"{}").unwrap();
        std::fs::set_permissions(
            dir.join("settings.json"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();

        let _s = Store::new(dir.clone());

        let perm = std::fs::metadata(dir.join("settings.json"))
            .unwrap()
            .permissions();
        assert_eq!(perm.mode() & 0o777, 0o600);
    }

    #[test]
    fn corrupt_settings_reads_as_defaults() {
        let s = temp_store();
        std::fs::create_dir_all(s.dir_for_test()).unwrap();
        std::fs::write(s.dir_for_test().join("settings.json"), b"not json").unwrap();
        let def = s.settings();
        assert_eq!(def.github_token, "");
        assert!(def.sources.brew); // corrupt -> all sources on, no panic
        assert!(def.probe_manual); // corrupt -> probing on (default), no panic
        assert!(def.advisory_checks); // corrupt -> advisory checks on (default), no panic
    }

    #[test]
    fn partial_settings_keeps_other_sources_on() {
        // A settings.json that only disables npm must keep brew/pip/npx/manual on,
        // never drop the unspecified sources to false.
        let s = temp_store();
        std::fs::create_dir_all(s.dir_for_test()).unwrap();
        std::fs::write(
            s.dir_for_test().join("settings.json"),
            br#"{"sources":{"npm":false}}"#,
        )
        .unwrap();
        let got = s.settings();
        assert!(!got.sources.npm);
        assert!(got.sources.brew && got.sources.pip && got.sources.npx && got.sources.manual);
        assert_eq!(got.github_token, "");
    }

    #[test]
    fn old_settings_file_without_probe_manual_key_defaults_to_true() {
        // Simulates a settings.json written before this field existed: no
        // "probeManual" key at all. It must still parse, and probing must
        // default on so upgrading users see no behavior change.
        let s = temp_store();
        std::fs::create_dir_all(s.dir_for_test()).unwrap();
        std::fs::write(
            s.dir_for_test().join("settings.json"),
            br#"{"githubToken":"abc","sources":{"npm":true,"brew":true,"pip":true,"npx":true,"manual":true}}"#,
        )
        .unwrap();
        let got = s.settings();
        assert_eq!(got.github_token, "abc");
        assert!(got.probe_manual);
    }

    #[test]
    fn old_settings_file_without_advisory_checks_key_defaults_to_true() {
        // Simulates a settings.json written before this field existed: no
        // "advisoryChecks" key at all. It must still parse, and the advisory
        // scan must default on so upgrading users see no behavior change.
        let s = temp_store();
        std::fs::create_dir_all(s.dir_for_test()).unwrap();
        std::fs::write(
            s.dir_for_test().join("settings.json"),
            br#"{"githubToken":"abc","sources":{"npm":true,"brew":true,"pip":true,"npx":true,"manual":true},"probeManual":true}"#,
        )
        .unwrap();
        let got = s.settings();
        assert_eq!(got.github_token, "abc");
        assert!(got.advisory_checks);
    }

    #[test]
    fn migrate_copies_user_files_when_target_missing() {
        let legacy = temp_store();
        std::fs::write(legacy.dir_for_test().join("history.json"), b"[{\"old\":1}]").unwrap();
        std::fs::write(legacy.dir_for_test().join("pins.json"), b"[\"typescript\"]").unwrap();
        // a cache file that must NOT be migrated
        std::fs::write(legacy.dir_for_test().join("wire.json"), b"{}").unwrap();

        let mut current = std::env::temp_dir();
        current.push(format!("napm-test-cur-{:p}", &legacy));
        let _ = std::fs::remove_dir_all(&current);

        migrate_legacy(&current, &legacy.dir_for_test());

        assert_eq!(
            std::fs::read(current.join("history.json")).unwrap(),
            b"[{\"old\":1}]"
        );
        assert!(current.join("pins.json").exists());
        assert!(!current.join("wire.json").exists()); // caches not migrated
    }

    #[test]
    fn migrate_never_clobbers_existing() {
        let legacy = temp_store();
        std::fs::write(legacy.dir_for_test().join("history.json"), b"LEGACY").unwrap();

        let mut current = std::env::temp_dir();
        current.push(format!("napm-test-cur2-{:p}", &legacy));
        let _ = std::fs::remove_dir_all(&current);
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("history.json"), b"CURRENT").unwrap();

        migrate_legacy(&current, &legacy.dir_for_test());

        assert_eq!(
            std::fs::read(current.join("history.json")).unwrap(),
            b"CURRENT"
        );
    }

    #[test]
    fn migrate_absent_legacy_is_noop() {
        let mut legacy = std::env::temp_dir();
        legacy.push(format!("napm-test-missing-{:p}", &legacy));
        let _ = std::fs::remove_dir_all(&legacy);
        let mut current = std::env::temp_dir();
        current.push(format!("napm-test-cur3-{:p}", &current));
        let _ = std::fs::remove_dir_all(&current);

        migrate_legacy(&current, &legacy); // must not panic
        assert!(!current.join("history.json").exists());
    }
}
