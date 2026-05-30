use serde::Serialize;

pub mod npm;

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

/// Aggregate across all sources. Milestone 1: npm only.
pub fn scan_all() -> Vec<InstalledTool> {
    npm::scan_npm()
}
