use serde::Serialize;
use std::process::Command;

pub mod npm;
pub mod brew;
pub mod pip;
pub mod npx;

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
