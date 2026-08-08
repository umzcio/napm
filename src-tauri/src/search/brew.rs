use super::SearchResult;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// A catalog formula reduced to the fields search needs, with name and
/// description pre-lowercased so the per-query substring match does no repeated
/// case folding.
#[derive(Debug, Clone, PartialEq)]
pub struct Formula {
    pub name: String,
    pub name_lc: String,
    pub desc: String,
    pub desc_lc: String,
    pub version: String,
}

/// Parse the brew `formula.json` catalog into the lightweight Formula form.
pub fn parse_catalog(json: &str) -> Vec<Formula> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match v.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut out = Vec::with_capacity(arr.len());
    for f in arr {
        let name = f.get("name").and_then(|x| x.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let desc = f.get("desc").and_then(|x| x.as_str()).unwrap_or("").trim();
        let version = f
            .get("versions")
            .and_then(|x| x.get("stable"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        out.push(Formula {
            name: name.to_string(),
            name_lc: name.to_lowercase(),
            desc: desc.to_string(),
            desc_lc: desc.to_lowercase(),
            version,
        });
    }
    out
}

/// formula name -> 30-day install count. The API gives `count` as a
/// comma-grouped string ("1,234,567"), so strip commas before parsing.
pub fn parse_analytics(json: &str) -> BTreeMap<String, u64> {
    let mut map = BTreeMap::new();
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return map,
    };
    let formulae = match v.get("formulae").and_then(|f| f.as_object()) {
        Some(o) => o,
        None => return map,
    };
    for (name, arr) in formulae {
        let count = arr
            .as_array()
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

/// Search the parsed catalog in-process. Case-insensitive substring on name or
/// description (both already lowercased). weekly_downloads is the 30-day
/// analytics count divided to a rough weekly figure.
pub fn search_parsed(
    formulae: &[Formula],
    query: &str,
    analytics: &BTreeMap<String, u64>,
) -> Vec<SearchResult> {
    let q = query.to_lowercase();
    let mut out = Vec::new();
    for f in formulae {
        if !f.name_lc.contains(&q) && !f.desc_lc.contains(&q) {
            continue;
        }
        let weekly = analytics.get(&f.name).copied().unwrap_or(0) / 4;
        out.push(SearchResult {
            name: f.name.clone(),
            eco: "brew".into(),
            pkg: f.name.clone(),
            version: f.version.clone(),
            weekly_downloads: weekly,
            size: String::new(),
            description: f.desc.clone(),
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
    match crate::http::get(url) {
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

/// Parsed catalog + analytics held in memory so the multi-MB JSON is parsed
/// once per process, not once per search. `loaded` drives the same 24h refresh
/// as the disk cache. The inner data is Arc'd so a search clones cheaply and
/// runs its substring scan outside the lock.
struct CatalogCache {
    loaded: SystemTime,
    formulae: Arc<Vec<Formula>>,
    analytics: Arc<BTreeMap<String, u64>>,
}

fn catalog_cell() -> &'static Mutex<Option<CatalogCache>> {
    static CELL: OnceLock<Mutex<Option<CatalogCache>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// Catalog formulae plus their weekly-install analytics, returned as cheap Arc clones.
type CatalogAndAnalytics = (Arc<Vec<Formula>>, Arc<BTreeMap<String, u64>>);

/// Load the parsed catalog and analytics, using the in-memory copy when it is
/// under 24h old, otherwise rebuilding it from `cached_or_fetch` (which itself
/// keeps a 24h disk cache). Returns cheap Arc clones. Returns None only when the
/// catalog cannot be obtained at all (no memory copy, no disk copy, no network).
fn load_catalog(cache_dir: &Path) -> Option<CatalogAndAnalytics> {
    {
        let guard = catalog_cell().lock().unwrap();
        if let Some(c) = guard.as_ref() {
            let fresh = SystemTime::now()
                .duration_since(c.loaded)
                .unwrap_or(Duration::MAX)
                < Duration::from_secs(24 * 60 * 60);
            if fresh && !c.formulae.is_empty() {
                return Some((c.formulae.clone(), c.analytics.clone()));
            }
        }
    }

    let catalog_json = cached_or_fetch(
        &cache_dir.join("brew_catalog.json"),
        "https://formulae.brew.sh/api/formula.json",
    )?;
    let analytics_map = cached_or_fetch(
        &cache_dir.join("brew_analytics.json"),
        "https://formulae.brew.sh/api/analytics/install/30d.json",
    )
    .map(|s| parse_analytics(&s))
    .unwrap_or_default();

    let formulae = Arc::new(parse_catalog(&catalog_json));
    let analytics = Arc::new(analytics_map);

    let mut guard = catalog_cell().lock().unwrap();
    *guard = Some(CatalogCache {
        loaded: SystemTime::now(),
        formulae: formulae.clone(),
        analytics: analytics.clone(),
    });
    Some((formulae, analytics))
}

/// Load and cache the brew catalog without searching. Called in a background
/// thread at launch so the first user search is warm. Best-effort: errors are
/// swallowed by the caller.
pub fn warm_brew(cache_dir: &Path) {
    let _ = load_catalog(cache_dir);
}

/// Drop the in-memory parsed catalog so the next load re-reads or re-fetches.
/// Called when the user clears the on-disk caches, otherwise the fresh
/// in-memory copy would mask the deletion until restart.
pub fn invalidate_catalog() {
    if let Ok(mut guard) = catalog_cell().lock() {
        *guard = None;
    }
}

/// Search brew formulae using the in-memory parsed catalog (backed by a 24h
/// disk cache and analytics). If the catalog cannot be obtained at all, returns
/// an empty list so brew is simply absent rather than an error.
pub fn search_brew(query: &str, cache_dir: &Path) -> Vec<SearchResult> {
    let (formulae, analytics) = match load_catalog(cache_dir) {
        Some(pair) => pair,
        None => return Vec::new(),
    };
    search_parsed(&formulae, query, &analytics)
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
    fn parse_catalog_reduces_and_lowercases() {
        let catalog = r#"[
            {"name":"RipGrep","desc":"Recursive Search","versions":{"stable":"14.1.1"}},
            {"name":"","desc":"skip me","versions":{"stable":"1.0"}}
        ]"#;
        let f = parse_catalog(catalog);
        assert_eq!(f.len(), 1); // empty-name entry skipped
        assert_eq!(f[0].name, "RipGrep");
        assert_eq!(f[0].name_lc, "ripgrep");
        assert_eq!(f[0].desc_lc, "recursive search");
        assert_eq!(f[0].version, "14.1.1");
    }

    #[test]
    fn substring_match_on_name_or_desc_with_weekly_from_analytics() {
        let formulae = parse_catalog(
            r#"[
            {"name":"ripgrep","desc":"Recursive search faster than grep","versions":{"stable":"14.1.1"}},
            {"name":"jq","desc":"JSON processor","versions":{"stable":"1.7.1"}}
        ]"#,
        );
        let mut a = BTreeMap::new();
        a.insert("ripgrep".to_string(), 4_000_000u64);
        let hits = search_parsed(&formulae, "search", &a);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].pkg, "ripgrep");
        assert_eq!(hits[0].eco, "brew");
        assert_eq!(hits[0].version, "14.1.1");
        assert_eq!(hits[0].weekly_downloads, 1_000_000); // 4M / 4
                                                         // matches description too:
        assert_eq!(search_parsed(&formulae, "json", &a).len(), 1);
    }
}
