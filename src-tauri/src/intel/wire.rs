use super::WireItem;
use serde_json::Value;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Parse a GitHub global-advisories array into wire items, tagging each with eco.
pub fn parse_advisories(json: &str, eco: &str) -> Vec<WireItem> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match v.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .filter_map(|a| {
            let id = a.get("ghsa_id").and_then(|x| x.as_str())?;
            let summary = a
                .get("summary")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let published = a
                .get("published_at")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let link = a
                .get("html_url")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let packages = a
                .get("vulnerabilities")
                .and_then(|x| x.as_array())
                .map(|vs| {
                    vs.iter()
                        .filter_map(|vuln| {
                            vuln.get("package")
                                .and_then(|p| p.get("name"))
                                .and_then(|n| n.as_str())
                                .map(String::from)
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(WireItem {
                id: id.to_string(),
                eco: eco.to_string(),
                summary,
                packages,
                published,
                link,
            })
        })
        .collect()
}

/// Merge per-source fetch results with the cached items. For a source that
/// failed, fall back to that source's items from `cached`. `complete` is true
/// only when both fetches succeeded. Result is sorted by published descending
/// and capped to 15 items.
fn merge_wire(
    npm: Option<Vec<WireItem>>,
    pip: Option<Vec<WireItem>>,
    cached: &[WireItem],
) -> (Vec<WireItem>, bool) {
    let complete = npm.is_some() && pip.is_some();

    let mut merged: Vec<WireItem> = Vec::new();
    match npm {
        Some(items) => merged.extend(items),
        None => merged.extend(cached.iter().filter(|w| w.eco == "npm").cloned()),
    }
    match pip {
        Some(items) => merged.extend(items),
        None => merged.extend(cached.iter().filter(|w| w.eco == "pip").cloned()),
    }

    // Sort by published descending (ISO timestamps sort correctly as strings).
    // Items with an empty published string sort LAST (treat "" as oldest).
    merged.sort_by(|a, b| {
        let key = |w: &WireItem| (!w.published.is_empty(), w.published.clone());
        key(b).cmp(&key(a))
    });
    merged.truncate(15);

    (merged, complete)
}

/// Fetch npm and pip malware advisories from GitHub, merge (npm first), sort by
/// published descending, cap to 15, and cache as wire.json in cache_dir with a
/// 1h freshness window. Returns None only if both fetches fail and no stale cache
/// exists. The returned bool is `complete`: true only when both sources were
/// fetched fresh this call. A partial result (complete == false) is never
/// written to the cache, so a stale source can still backfill next time.
pub fn fetch_wire(cache_dir: &Path) -> Option<(Vec<WireItem>, bool)> {
    let cache_path = cache_dir.join("wire.json");

    // Check freshness: under 1h is a live hit.
    let fresh = std::fs::metadata(&cache_path)
        .and_then(|m| m.modified())
        .ok()
        .map(|mtime| {
            SystemTime::now()
                .duration_since(mtime)
                .unwrap_or(Duration::MAX)
                < Duration::from_secs(60 * 60)
        })
        .unwrap_or(false);

    if fresh {
        if let Ok(text) = std::fs::read_to_string(&cache_path) {
            if let Ok(items) = serde_json::from_str::<Vec<WireItem>>(&text) {
                return Some((items, true));
            }
        }
    }

    // Whatever is on disk, fresh or stale, backfills a source whose fetch fails.
    let cached: Vec<WireItem> = std::fs::read_to_string(&cache_path)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<WireItem>>(&t).ok())
        .unwrap_or_default();

    // Stale or missing: attempt fresh fetches.
    let npm_url =
        "https://api.github.com/advisories?type=malware&ecosystem=npm&sort=published&per_page=15";
    let pip_url =
        "https://api.github.com/advisories?type=malware&ecosystem=pip&sort=published&per_page=15";

    // Build auth header string bindings so references live long enough.
    let token_str: String;
    let token_header: String;
    let mut base_headers: Vec<(&str, &str)> = vec![("Accept", "application/vnd.github+json")];
    if let Some(token) = super::github_token(cache_dir) {
        token_str = token;
        token_header = format!("Bearer {}", token_str);
        base_headers.push(("Authorization", &token_header));
    }

    let npm_result = crate::http::get_with_headers(npm_url, &base_headers);
    let pip_result = crate::http::get_with_headers(pip_url, &base_headers);

    if npm_result.is_err() && pip_result.is_err() {
        // Both failed: return stale cache if available, else None.
        return if cached.is_empty() {
            None
        } else {
            Some((cached, false))
        };
    }

    let npm_items = npm_result.ok().map(|body| parse_advisories(&body, "npm"));
    let pip_items = pip_result.ok().map(|body| parse_advisories(&body, "pip"));

    let (merged, complete) = merge_wire(npm_items, pip_items, &cached);

    // A partial result must not poison the cache: only a complete fetch is
    // written, so the stale cache remains available to backfill next time.
    if complete {
        if let Ok(text) = serde_json::to_string(&merged) {
            let _ = std::fs::write(&cache_path, &text);
        }
    }

    Some((merged, complete))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_advisory_array_with_packages() {
        let json = r#"[
          {"ghsa_id":"GHSA-1","summary":"Malware in foo","published_at":"2026-05-19T00:00:00Z",
           "html_url":"https://github.com/advisories/GHSA-1",
           "vulnerabilities":[{"package":{"ecosystem":"npm","name":"foo"}},{"package":{"ecosystem":"npm","name":"bar"}}]}
        ]"#;
        let w = parse_advisories(json, "npm");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].id, "GHSA-1");
        assert_eq!(w[0].eco, "npm");
        assert_eq!(w[0].packages, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn garbage_is_empty() {
        assert!(parse_advisories("nope", "npm").is_empty());
    }

    fn item(id: &str, eco: &str, published: &str) -> WireItem {
        WireItem {
            id: id.to_string(),
            eco: eco.to_string(),
            summary: format!("summary for {id}"),
            packages: Vec::new(),
            published: published.to_string(),
            link: String::new(),
        }
    }

    #[test]
    fn merge_wire_both_fresh_is_complete() {
        let npm = vec![item("GHSA-npm-1", "npm", "2026-01-02T00:00:00Z")];
        let pip = vec![item("GHSA-pip-1", "pip", "2026-01-01T00:00:00Z")];
        let (merged, complete) = merge_wire(Some(npm), Some(pip), &[]);
        assert!(complete);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "GHSA-npm-1");
        assert_eq!(merged[1].id, "GHSA-pip-1");
    }

    #[test]
    fn merge_wire_npm_failed_falls_back_to_cached_npm() {
        let cached = vec![
            item("GHSA-npm-old", "npm", "2025-12-01T00:00:00Z"),
            item("GHSA-pip-old", "pip", "2025-11-01T00:00:00Z"),
        ];
        let pip = vec![item("GHSA-pip-new", "pip", "2026-01-01T00:00:00Z")];
        let (merged, complete) = merge_wire(None, Some(pip), &cached);
        assert!(!complete);
        let ids: Vec<&str> = merged.iter().map(|w| w.id.as_str()).collect();
        assert!(ids.contains(&"GHSA-npm-old"));
        assert!(ids.contains(&"GHSA-pip-new"));
        assert!(!ids.contains(&"GHSA-pip-old"));
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_wire_npm_failed_empty_cache_is_pip_only() {
        let pip = vec![item("GHSA-pip-new", "pip", "2026-01-01T00:00:00Z")];
        let (merged, complete) = merge_wire(None, Some(pip), &[]);
        assert!(!complete);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "GHSA-pip-new");
    }

    #[test]
    fn merge_wire_both_failed_falls_back_to_cache_entirely() {
        let cached = vec![
            item("GHSA-npm-old", "npm", "2025-12-01T00:00:00Z"),
            item("GHSA-pip-old", "pip", "2025-11-01T00:00:00Z"),
        ];
        let (merged, complete) = merge_wire(None, None, &cached);
        assert!(!complete);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_wire_sorts_desc_and_truncates_to_15() {
        // npm items are all older (2025) than pip items (2026), and there are
        // more pip items than the 15-item cap, so the truncated result should
        // be entirely pip, newest first.
        let npm: Vec<WireItem> = (0..5)
            .map(|i| {
                item(
                    &format!("GHSA-npm-{i}"),
                    "npm",
                    &format!("2025-01-{:02}T00:00:00Z", i + 1),
                )
            })
            .collect();
        let pip: Vec<WireItem> = (0..20)
            .map(|i| {
                item(
                    &format!("GHSA-pip-{i}"),
                    "pip",
                    &format!("2026-02-{:02}T00:00:00Z", i + 1),
                )
            })
            .collect();
        let (merged, complete) = merge_wire(Some(npm), Some(pip), &[]);
        assert!(complete);
        assert_eq!(merged.len(), 15);
        // Descending order: the newest pip item (day 20) sorts first.
        assert_eq!(merged[0].id, "GHSA-pip-19");
        for w in &merged {
            assert!(
                w.eco == "pip",
                "expected all top-15 to be pip items, got {}",
                w.eco
            );
        }
    }
}
