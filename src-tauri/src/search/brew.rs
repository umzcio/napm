use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime};
use serde_json::Value;
use super::SearchResult;

/// formula name -> 30-day install count. The API gives `count` as a
/// comma-grouped string ("1,234,567"), so strip commas before parsing.
pub fn parse_analytics(json: &str) -> BTreeMap<String, u64> {
    let mut map = BTreeMap::new();
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return map };
    let formulae = match v.get("formulae").and_then(|f| f.as_object()) {
        Some(o) => o, None => return map,
    };
    for (name, arr) in formulae {
        let count = arr.as_array()
            .and_then(|a| a.first())
            .and_then(|e| e.get("count"))
            .and_then(|c| c.as_str())
            .map(|s| s.replace(',', ""))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        map.insert(name.clone(), count);
    }
    map
}

/// Search the cached brew catalog in-process. Case-insensitive substring on
/// name or description. weekly_downloads is the 30-day analytics count divided
/// to a rough weekly figure (labeled approximate in the UI).
pub fn search_catalog(catalog_json: &str, query: &str, analytics: &BTreeMap<String, u64>) -> Vec<SearchResult> {
    let v: Value = match serde_json::from_str(catalog_json) { Ok(v) => v, Err(_) => return Vec::new() };
    let arr = match v.as_array() { Some(a) => a, None => return Vec::new() };
    let q = query.to_lowercase();
    let mut out = Vec::new();
    for f in arr {
        let name = f.get("name").and_then(|x| x.as_str()).unwrap_or("");
        if name.is_empty() { continue; }
        let desc = f.get("desc").and_then(|x| x.as_str()).unwrap_or("");
        if !name.to_lowercase().contains(&q) && !desc.to_lowercase().contains(&q) { continue; }
        let version = f.get("versions").and_then(|x| x.get("stable"))
            .and_then(|x| x.as_str()).unwrap_or("").to_string();
        let weekly = analytics.get(name).copied().unwrap_or(0) / 4;
        out.push(SearchResult {
            name: name.to_string(), eco: "brew".into(), pkg: name.to_string(),
            version, weekly_downloads: weekly, size: String::new(),
            description: desc.trim().to_string(),
        });
    }
    out
}

/// Return cached file content if it is under 24h old, otherwise fetch `url`,
/// write the result to `path`, and return the new content. Returns None if
/// the fetch fails and no cached copy exists.
fn cached_or_fetch(path: &Path, url: &str) -> Option<String> {
    let fresh = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(|mtime| {
            SystemTime::now()
                .duration_since(mtime)
                .unwrap_or(Duration::MAX)
                < Duration::from_secs(24 * 60 * 60)
        })
        .unwrap_or(false);

    if fresh {
        return std::fs::read_to_string(path).ok();
    }

    // Stale or missing: attempt a fresh fetch.
    match super::http::get(url) {
        Ok(body) => {
            // Best-effort write; if it fails the caller still gets the body.
            let _ = std::fs::write(path, &body);
            Some(body)
        }
        Err(_) => {
            // Fall back to any stale cached copy rather than returning nothing.
            std::fs::read_to_string(path).ok()
        }
    }
}

/// Search brew formulae using a locally cached catalog and install analytics.
/// The catalog and analytics files are cached for 24h in `cache_dir`.
/// If both fetches fail and no cache exists, returns an empty list.
pub fn search_brew(query: &str, cache_dir: &Path) -> Vec<SearchResult> {
    let catalog_path = cache_dir.join("brew_catalog.json");
    let analytics_path = cache_dir.join("brew_analytics.json");

    let catalog_json = match cached_or_fetch(
        &catalog_path,
        "https://formulae.brew.sh/api/formula.json",
    ) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let analytics_map = cached_or_fetch(
        &analytics_path,
        "https://formulae.brew.sh/api/analytics/install/30d.json",
    )
    .map(|s| parse_analytics(&s))
    .unwrap_or_default();

    search_catalog(&catalog_json, query, &analytics_map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comma_grouped_counts() {
        let json = r#"{"formulae":{
            "wget":[{"formula":"wget","count":"1,234,567"}],
            "jq":[{"formula":"jq","count":"890"}]
        }}"#;
        let m = parse_analytics(json);
        assert_eq!(m.get("wget"), Some(&1234567));
        assert_eq!(m.get("jq"), Some(&890));
    }

    #[test]
    fn substring_match_on_name_or_desc_with_weekly_from_analytics() {
        let catalog = r#"[
            {"name":"ripgrep","desc":"Recursive search faster than grep","versions":{"stable":"14.1.1"}},
            {"name":"jq","desc":"JSON processor","versions":{"stable":"1.7.1"}}
        ]"#;
        let mut a = BTreeMap::new();
        a.insert("ripgrep".to_string(), 4_000_000u64);
        let hits = search_catalog(catalog, "search", &a);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].pkg, "ripgrep");
        assert_eq!(hits[0].eco, "brew");
        assert_eq!(hits[0].version, "14.1.1");
        assert_eq!(hits[0].weekly_downloads, 1_000_000); // 4M / 4
        // matches description too:
        assert_eq!(search_catalog(catalog, "json", &a).len(), 1);
    }
}
