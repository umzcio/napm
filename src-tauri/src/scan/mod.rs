use crate::store::Sources;
use serde::Serialize;
use std::process::Command;

pub mod brew;
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
/// Only sources enabled in `sources` are scanned.
pub fn scan_all(pins: &std::collections::BTreeSet<String>, sources: Sources) -> Vec<InstalledTool> {
    let mut all = Vec::new();
    if sources.npm {
        all.extend(npm::scan_npm());
    }
    if sources.brew {
        all.extend(brew::scan_brew());
    }
    if sources.pip {
        all.extend(pip::scan_pip());
    }
    if sources.npx {
        all.extend(npx::scan_npx());
    }
    if sources.manual {
        let other_names: std::collections::BTreeSet<String> =
            all.iter().map(|t| t.name.clone()).collect();
        all.extend(manual::scan_manual(&other_names));
    }
    for row in all.iter_mut() {
        row.pinned = pins.contains(&row.pkg);
    }
    stamp_status_and_bump(&mut all);
    all
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
            manual: false,
        };
        assert_eq!(scan_all(&pins, sources), Vec::new());
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
