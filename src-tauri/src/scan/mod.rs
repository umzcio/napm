use serde::Serialize;
use std::process::Command;

pub mod npm;
pub mod brew;
pub mod pip;
pub mod npx;
pub mod publisher;
pub mod size;

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

/// Aggregate across all sources. A source whose tool is absent contributes
/// nothing (its scanner returns an empty Vec).
pub fn scan_all() -> Vec<InstalledTool> {
    let mut all = Vec::new();
    all.extend(npm::scan_npm());
    all.extend(brew::scan_brew());
    all.extend(pip::scan_pip());
    all.extend(npx::scan_npx());
    all
}
