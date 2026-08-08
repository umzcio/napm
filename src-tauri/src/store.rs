use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub github_token: String,
    pub sources: Sources,
}

/// Flat-JSON persistence for pins and history in a single directory.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        let _ = std::fs::create_dir_all(&dir);
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

    fn write_json<T: Serialize>(path: &Path, value: &T) {
        if let Ok(s) = serde_json::to_string_pretty(value) {
            let _ = std::fs::write(path, s);
        }
    }

    pub fn pins(&self) -> BTreeSet<String> {
        Self::read_json(&self.pins_path())
    }

    pub fn set_pin(&self, pkg: &str, on: bool) {
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
        Self::write_json(&self.settings_path(), s);
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
        s.set_settings(&Settings {
            github_token: "abc".into(),
            sources: Sources {
                npm: true,
                brew: false,
                pip: true,
                npx: true,
                manual: true,
            },
        });
        let got = s.settings();
        assert_eq!(got.github_token, "abc");
        assert!(!got.sources.brew);
        assert!(got.sources.npm && got.sources.pip);
    }

    #[test]
    fn corrupt_settings_reads_as_defaults() {
        let s = temp_store();
        std::fs::create_dir_all(s.dir_for_test()).unwrap();
        std::fs::write(s.dir_for_test().join("settings.json"), b"not json").unwrap();
        let def = s.settings();
        assert_eq!(def.github_token, "");
        assert!(def.sources.brew); // corrupt -> all sources on, no panic
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
