use serde::Serialize;
use std::path::Path;

pub mod npm;
pub mod brew;
pub mod pip;

/// One discovered package in the swarm. Canonical SearchResult shape.
/// Serialized camelCase so the frontend reads `weeklyDownloads`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub name: String,
    pub eco: String,
    pub pkg: String,
    pub version: String,
    pub weekly_downloads: u64,
    pub size: String,        // "" when the registry does not expose install size
    pub description: String,
}

/// Pool results from all sources, dedupe by (eco, pkg) keeping the first seen,
/// and sort by weekly_downloads descending (the trust signal / sort key).
pub fn merge(sources: Vec<Vec<SearchResult>>) -> Vec<SearchResult> {
    use std::collections::BTreeSet;
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut out: Vec<SearchResult> = Vec::new();
    for source in sources {
        for r in source {
            if seen.insert((r.eco.clone(), r.pkg.clone())) {
                out.push(r);
            }
        }
    }
    out.sort_by(|a, b| b.weekly_downloads.cmp(&a.weekly_downloads));
    out
}

/// Federated swarm search: npm + brew + pip, merged and sorted. Each source
/// fails independently to an empty list, so one dead registry never blanks the
/// grid. `cache_dir` holds the brew catalog/analytics caches.
pub fn search_all(query: &str, cache_dir: &Path) -> Vec<SearchResult> {
    let query = query.trim();
    if query.is_empty() { return Vec::new(); }
    // Fan out the three sources concurrently: total latency becomes the slowest
    // source, not the sum. A panicking source thread degrades to an empty list,
    // so one dead registry still never blanks the grid.
    std::thread::scope(|s| {
        let n = s.spawn(|| npm::search_npm(query));
        let b = s.spawn(|| brew::search_brew(query, cache_dir));
        let p = s.spawn(|| pip::search_pip(query));
        merge(vec![
            n.join().unwrap_or_default(),
            b.join().unwrap_or_default(),
            p.join().unwrap_or_default(),
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(eco: &str, pkg: &str, dl: u64) -> SearchResult {
        SearchResult { name: pkg.into(), eco: eco.into(), pkg: pkg.into(),
            version: "1.0.0".into(), weekly_downloads: dl, size: String::new(),
            description: String::new() }
    }

    #[test]
    fn merge_sorts_by_downloads_desc_and_dedupes() {
        let merged = merge(vec![
            vec![r("npm", "a", 100), r("brew", "b", 500)],
            vec![r("npm", "a", 999), r("pip", "c", 300)], // dup (npm,a) dropped
        ]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].pkg, "b");   // 500
        assert_eq!(merged[1].pkg, "c");   // 300
        assert_eq!(merged[2].pkg, "a");   // 100 (first-seen kept)
    }
}

