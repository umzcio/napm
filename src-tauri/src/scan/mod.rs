use crate::store::Sources;
use serde::Serialize;
use std::process::Command;

pub mod brew;
pub mod cargo;
pub mod manual;
pub mod npm;
pub mod npx;
pub mod pip;
pub mod publisher;
pub mod size;
pub mod version;

/// One row in the Shared Library. Mirrors the prototype's tool shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstalledTool {
    pub name: String,
    pub eco: String,
    pub pkg: String,
    pub installed: Option<String>,
    pub latest: String,
    pub size: String,
    pub pinned: bool,
    /// Real package publisher as a lowercase handle (e.g. "anthropic"), or ""
    /// when the local metadata has no author. Rendered as the "Shared By" column.
    pub publisher: String,
    /// One-line description/summary from the package metadata, or "".
    pub description: String,
    /// When the installed files last changed on disk, as a Unix timestamp in
    /// seconds (brew's recorded install time where available, else folder
    /// mtime). 0 when unknown. The frontend renders this relatively.
    pub updated: i64,
    /// True when the user explicitly asked for this tool (npm/pip/npx globals are
    /// always user-chosen; for brew this is installed_on_request from the receipt).
    /// Unknown defaults to true so "only tools I installed" never wrongly hides a tool.
    pub requested: bool,
    /// Derived library status: "unmanaged", "offline", "current", or "update".
    /// Stamped by `scan_all` via `version::status_of`; the frontend only reads it.
    pub status: String,
    /// Derived bump size for an available update: "major", "minor", "patch",
    /// or "none". Stamped by `scan_all` via `version::bump_kind`.
    pub bump: String,
}

/// Run a command and return its stdout, ignoring exit status (some tools, like
/// `npm outdated`, exit non-zero when they have results). Returns "" on spawn
/// failure, so a missing tool degrades to an empty scan rather than an error.
pub(crate) fn run(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// The modification time of a path as a Unix timestamp in seconds, or 0 if
/// unavailable. Used as the "last changed on disk" signal for the Updated column.
pub(crate) fn path_mtime(path: &std::path::Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Aggregate across all sources, marking rows whose pkg is in `pins`.
/// Only sources enabled in `sources` are scanned. `probe_manual` and
/// `cache_dir` are passed through to the manual scanner: they gate and cache
/// its `<tool> --version` probing of $HOME binaries (see `manual::scan_manual`).
pub fn scan_all(
    pins: &std::collections::BTreeSet<String>,
    sources: Sources,
    probe_manual: bool,
    cache_dir: &std::path::Path,
) -> Vec<InstalledTool> {
    // Fan out the five independent scanners concurrently, mirroring
    // search::search_all: four of them block on a package-manager network
    // call in a subprocess (`npm outdated`, `brew outdated`, `pip list
    // --outdated`, crates.io lookups for cargo), so running them in parallel
    // makes total latency the slowest source rather than the sum. manual must
    // run AFTER the join: it needs `other_names` built from the other five
    // sources' results, so it stays sequential. A panicking source thread
    // degrades to an empty vec, matching each scanner's own no-op-on-failure
    // behavior.
    let (npm_rows, brew_rows, pip_rows, npx_rows, (cargo_rows, cargo_bins)) =
        std::thread::scope(|s| {
            let n = s.spawn(|| {
                if sources.npm {
                    npm::scan_npm()
                } else {
                    Vec::new()
                }
            });
            let b = s.spawn(|| {
                if sources.brew {
                    brew::scan_brew()
                } else {
                    Vec::new()
                }
            });
            let p = s.spawn(|| {
                if sources.pip {
                    pip::scan_pip()
                } else {
                    Vec::new()
                }
            });
            let x = s.spawn(|| {
                if sources.npx {
                    npx::scan_npx()
                } else {
                    Vec::new()
                }
            });
            let c = s.spawn(|| {
                if sources.cargo {
                    cargo::scan_cargo_with_bins(cache_dir)
                } else {
                    (Vec::new(), Vec::new())
                }
            });
            (
                n.join().unwrap_or_default(),
                b.join().unwrap_or_default(),
                p.join().unwrap_or_default(),
                x.join().unwrap_or_default(),
                c.join().unwrap_or_default(),
            )
        });

    let mut all = Vec::new();
    all.extend(npm_rows);
    all.extend(brew_rows);
    all.extend(pip_rows);
    all.extend(npx_rows);
    all.extend(cargo_rows);

    if sources.manual {
        let names = other_names(&all, &cargo_bins);
        all.extend(manual::scan_manual(&names, probe_manual, cache_dir));
    }
    for row in all.iter_mut() {
        row.pinned = pins.contains(&row.pkg);
    }
    stamp_status_and_bump(&mut all);
    all
}

/// The exclusion set the manual scanner receives: every already-scanned row's
/// display name, plus `extra` -- names a source contributes beyond its own
/// rows. Only cargo does this today: a multi-binary crate is one row (its
/// crate name), but every binary it installs (e.g. `cargo-edit`'s
/// `cargo-add`/`cargo-rm`/...) must also be excluded from the manual $PATH
/// sweep, not just the crate's own name (see `cargo::scan_cargo_with_bins`).
fn other_names(rows: &[InstalledTool], extra: &[String]) -> std::collections::BTreeSet<String> {
    let mut set: std::collections::BTreeSet<String> = rows.iter().map(|t| t.name.clone()).collect();
    set.extend(extra.iter().cloned());
    set
}

/// Derive and set `status`/`bump` on every row from its own `eco`/`installed`/
/// `latest`. Shared by `scan_all` and its tests so the test exercises the
/// exact code path production runs.
fn stamp_status_and_bump(rows: &mut [InstalledTool]) {
    for row in rows.iter_mut() {
        row.status =
            version::status_of(&row.eco, row.installed.as_deref(), &row.latest).to_string();
        row.bump =
            version::bump_kind(row.installed.as_deref().unwrap_or(""), &row.latest).to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_all_with_all_sources_disabled_returns_empty() {
        let pins = std::collections::BTreeSet::new();
        let sources = Sources {
            npm: false,
            brew: false,
            pip: false,
            npx: false,
            cargo: false,
            manual: false,
        };
        assert_eq!(
            scan_all(&pins, sources, true, std::path::Path::new("/tmp")),
            Vec::new()
        );
    }

    #[test]
    fn other_names_folds_in_extra_names_beyond_rows() {
        // Mirrors a multi-binary crate (e.g. cargo-edit): one row named
        // "cargo-edit", but its OTHER binaries (cargo-add, cargo-rm, ...) must
        // also land in the manual scanner's exclusion set, or they leak in as
        // fake "manual" tools (the M9 regression class this guards against).
        let rows = vec![InstalledTool {
            name: "cargo-edit".to_string(),
            eco: "cargo".to_string(),
            pkg: "cargo-edit".to_string(),
            installed: Some("0.12.2".to_string()),
            latest: "0.12.2".to_string(),
            size: String::new(),
            pinned: false,
            publisher: String::new(),
            description: String::new(),
            updated: 0,
            requested: true,
            status: String::new(),
            bump: String::new(),
        }];
        let extra = vec![
            "cargo-add".to_string(),
            "cargo-rm".to_string(),
            "cargo-set-version".to_string(),
        ];
        let names = other_names(&rows, &extra);
        assert!(names.contains("cargo-edit"));
        assert!(names.contains("cargo-add"));
        assert!(names.contains("cargo-rm"));
        assert!(names.contains("cargo-set-version"));
    }

    #[test]
    fn stamp_status_and_bump_sets_derived_fields() {
        let mut rows = vec![InstalledTool {
            name: "foo".to_string(),
            eco: "npm".to_string(),
            pkg: "foo".to_string(),
            installed: Some("1.0.0".to_string()),
            latest: "1.2.0".to_string(),
            size: String::new(),
            pinned: false,
            publisher: String::new(),
            description: String::new(),
            updated: 0,
            requested: true,
            status: String::new(),
            bump: String::new(),
        }];
        stamp_status_and_bump(&mut rows);
        assert_eq!(rows[0].status, "update");
        assert_eq!(rows[0].bump, "minor");
    }
}
