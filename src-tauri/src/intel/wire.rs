use std::path::Path;
use std::time::{Duration, SystemTime};
use super::WireItem;
use serde_json::Value;

/// Parse a GitHub global-advisories array into wire items, tagging each with eco.
pub fn parse_advisories(json: &str, eco: &str) -> Vec<WireItem> {
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return Vec::new() };
    let arr = match v.as_array() { Some(a) => a, None => return Vec::new() };
    arr.iter().filter_map(|a| {
        let id = a.get("ghsa_id").and_then(|x| x.as_str())?;
        let summary = a.get("summary").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        let published = a.get("published_at").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let link = a.get("html_url").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let packages = a.get("vulnerabilities").and_then(|x| x.as_array()).map(|vs| {
            vs.iter().filter_map(|vuln| {
                vuln.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str()).map(String::from)
            }).collect()
        }).unwrap_or_default();
        Some(WireItem { id: id.to_string(), eco: eco.to_string(), summary, packages, published, link })
    }).collect()
}

/// Fetch npm and pip malware advisories from GitHub, merge (npm first), sort by
/// published descending, cap to 15, and cache as wire.json in cache_dir with a
/// 1h freshness window. Returns None only if both fetches fail and no stale cache
/// exists.
pub fn fetch_wire(cache_dir: &Path) -> Option<Vec<WireItem>> {
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
                return Some(items);
            }
        }
    }

    // Stale or missing: attempt fresh fetches.
    let npm_url = "https://api.github.com/advisories?type=malware&ecosystem=npm&sort=published&per_page=15";
    let pip_url = "https://api.github.com/advisories?type=malware&ecosystem=pip&sort=published&per_page=15";

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

    let npm_ok = npm_result.is_ok();
    let pip_ok = pip_result.is_ok();

    if !npm_ok && !pip_ok {
        // Both failed: return stale cache if available, else None.
        return std::fs::read_to_string(&cache_path)
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<WireItem>>(&t).ok());
    }

    let mut merged: Vec<WireItem> = Vec::new();

    if let Ok(body) = npm_result {
        merged.extend(parse_advisories(&body, "npm"));
    }
    if let Ok(body) = pip_result {
        merged.extend(parse_advisories(&body, "pip"));
    }

    // Sort by published descending (ISO timestamps sort correctly as strings).
    // Items with an empty published string sort LAST (treat "" as oldest).
    merged.sort_by(|a, b| {
        let key = |w: &WireItem| (!w.published.is_empty(), w.published.clone());
        key(b).cmp(&key(a))
    });
    merged.truncate(15);

    // Cache the merged vec.
    if let Ok(text) = serde_json::to_string(&merged) {
        let _ = std::fs::write(&cache_path, &text);
    }

    Some(merged)
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
    fn garbage_is_empty() { assert!(parse_advisories("nope", "npm").is_empty()); }
}
