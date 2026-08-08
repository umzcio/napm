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

/// Write `body` to `path` via a temp-file-then-rename, so a concurrent reader
/// (another process, or this one under a torn write) never observes a
/// partially written cache file. The temp file lives alongside `path` under
/// a `.tmp` extension so the rename stays on the same filesystem.
fn write_cache_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)
}

/// Return cached file content if it is under 24h old, otherwise fetch `url`,
/// write the result to `path` atomically, and return the new content.
/// Returns None if the fetch fails and no cached copy exists.
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
            let _ = write_cache_atomic(path, &body);
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

/// Serializes catalog fetch-and-parse across concurrent callers. Without
/// this, the startup warm thread and a user's first search can both miss
/// the (empty, or stale-by-mtime) in-memory cache and independently
/// download and parse the ~10 MB formula.json. Held across the network
/// fetch by design: cold-start brew searches queue behind one download
/// rather than each running it.
///
/// Lock ordering: this gate is always acquired BEFORE catalog_cell's lock
/// (see load_catalog). invalidate_catalog only ever locks catalog_cell and
/// never this gate, so no path acquires the two locks in the opposite
/// order.
fn fetch_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

/// True when a `CatalogCache` is present, under 24h old, and non-empty.
fn is_fresh(c: &CatalogCache) -> bool {
    let fresh = SystemTime::now()
        .duration_since(c.loaded)
        .unwrap_or(Duration::MAX)
        < Duration::from_secs(24 * 60 * 60);
    fresh && !c.formulae.is_empty()
}

/// Whether a just-parsed catalog is unusable and should trigger a
/// delete-and-refetch. A "fresh" (by mtime) disk file can still be corrupt
/// or torn, in which case parsing it yields zero formulae. Extracted as a
/// pure function so this decision is unit-testable without touching the
/// filesystem or network.
fn catalog_is_corrupt(formulae: &[Formula]) -> bool {
    formulae.is_empty()
}

/// Catalog formulae plus their weekly-install analytics, returned as cheap Arc clones.
type CatalogAndAnalytics = (Arc<Vec<Formula>>, Arc<BTreeMap<String, u64>>);

const CATALOG_URL: &str = "https://formulae.brew.sh/api/formula.json";

/// Load the parsed catalog and analytics, using the in-memory copy when it is
/// under 24h old, otherwise rebuilding it from `cached_or_fetch` (which itself
/// keeps a 24h disk cache). Returns cheap Arc clones. Returns None only when the
/// catalog cannot be obtained at all (no memory copy, no disk copy, no network).
fn load_catalog(cache_dir: &Path) -> Option<CatalogAndAnalytics> {
    {
        let guard = catalog_cell().lock().unwrap();
        if let Some(c) = guard.as_ref() {
            if is_fresh(c) {
                return Some((c.formulae.clone(), c.analytics.clone()));
            }
        }
    }

    // Only one thread fetches/parses at a time; everyone else queues here.
    let _fetch_permit = fetch_gate().lock().unwrap();

    // Re-check: another thread may have filled the cache while we waited
    // for the gate.
    {
        let guard = catalog_cell().lock().unwrap();
        if let Some(c) = guard.as_ref() {
            if is_fresh(c) {
                return Some((c.formulae.clone(), c.analytics.clone()));
            }
        }
    }

    let catalog_path = cache_dir.join("brew_catalog.json");
    let catalog_json = cached_or_fetch(&catalog_path, CATALOG_URL)?;
    let mut formulae = parse_catalog(&catalog_json);
    if catalog_is_corrupt(&formulae) {
        // The disk copy may be corrupt or torn (e.g. an interleaved write
        // from a past concurrent fetch, before this fetch was single-
        // flighted) and still counts as "fresh" by mtime alone. Delete it
        // and retry once so a bad file doesn't keep returning zero brew
        // results for the rest of its 24h freshness window.
        let _ = std::fs::remove_file(&catalog_path);
        let catalog_json = cached_or_fetch(&catalog_path, CATALOG_URL)?;
        formulae = parse_catalog(&catalog_json);
    }

    let analytics_map = cached_or_fetch(
        &cache_dir.join("brew_analytics.json"),
        "https://formulae.brew.sh/api/analytics/install/30d.json",
    )
    .map(|s| parse_analytics(&s))
    .unwrap_or_default();

    let formulae = Arc::new(formulae);
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

    /// Unique-per-test scratch dir under the OS temp dir, so parallel test
    /// threads never collide on the same cache files.
    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "napm-brew-test-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn write_cache_atomic_leaves_no_tmp_file_behind() {
        let dir = scratch_dir("atomic-write");
        let path = dir.join("cache.json");

        write_cache_atomic(&path, "hello").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        assert!(!path.with_extension("tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cached_or_fetch_short_circuits_to_disk_when_fresh_even_if_corrupt() {
        // A file just written has a fresh mtime, so cached_or_fetch must
        // return its content straight from disk without touching the
        // network. Prove that with a URL that would fail fast if dialed.
        let dir = scratch_dir("fresh-corrupt");
        let path = dir.join("garbage.json");
        std::fs::write(&path, "not valid json").unwrap();

        let got = cached_or_fetch(&path, "http://127.0.0.1:1/unreachable");
        assert_eq!(got.as_deref(), Some("not valid json"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_catalog_parse_triggers_the_retry_decision() {
        // A garbage or empty catalog body parses to zero formulae, which is
        // exactly the signal load_catalog uses to delete-and-refetch.
        assert!(catalog_is_corrupt(&parse_catalog("not valid json")));
        assert!(catalog_is_corrupt(&parse_catalog("[]")));

        let good = parse_catalog(
            r#"[{"name":"jq","desc":"JSON processor","versions":{"stable":"1.7.1"}}]"#,
        );
        assert!(!catalog_is_corrupt(&good));
    }

    #[test]
    fn fetch_gate_plus_recheck_lets_only_one_caller_do_the_work() {
        // Mirrors load_catalog's gate-then-recheck shape: N threads race for
        // the gate, but only the first one through finds the cache unfilled
        // and does the (simulated) fetch; everyone else's re-check hits.
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Arc;

        let fetch_count = Arc::new(AtomicUsize::new(0));
        let filled = Arc::new(AtomicBool::new(false));

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let fetch_count = fetch_count.clone();
                let filled = filled.clone();
                std::thread::spawn(move || {
                    let _permit = fetch_gate().lock().unwrap();
                    if !filled.load(Ordering::SeqCst) {
                        fetch_count.fetch_add(1, Ordering::SeqCst);
                        filled.store(true, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(fetch_count.load(Ordering::SeqCst), 1);
    }
}
