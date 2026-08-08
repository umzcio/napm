//! Short-TTL cache for registry documents (npm packuments, PyPI JSON docs).
//!
//! The full npm packument or PyPI project JSON doc is large and was being
//! re-fetched up to three times for the same package (release verdict,
//! changelog lookup, npx drift check). This module puts a single cache in
//! front of all three call sites: an in-process memory layer backed by a
//! disk layer (`regdoc_<eco>_<pkg>.json` in the app-data dir), freshness
//! judged by TTL (memory) and file mtime (disk). TTL is 1 hour.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a cached copy (memory or disk) is considered fresh.
const TTL: Duration = Duration::from_secs(3600);

type DocKey = (String, String);
type DocEntry = (Instant, Arc<String>);
type DocMap = HashMap<DocKey, DocEntry>;

fn docs_cell() -> &'static Mutex<DocMap> {
    static DOCS: OnceLock<Mutex<DocMap>> = OnceLock::new();
    DOCS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Drop every in-memory cached document. Called when the user clears the
/// on-disk caches, otherwise the fresh in-memory copy would mask the deletion
/// until restart (mirrors `search::brew::invalidate_catalog`).
pub fn invalidate() {
    if let Ok(mut guard) = docs_cell().lock() {
        guard.clear();
    }
}

fn memory_get(key: &DocKey) -> Option<String> {
    let guard = docs_cell().lock().ok()?;
    let (t, s) = guard.get(key)?;
    if t.elapsed() < TTL {
        Some((**s).clone())
    } else {
        None
    }
}

fn memory_put(key: &DocKey, body: &str) {
    if let Ok(mut guard) = docs_cell().lock() {
        guard.insert(key.clone(), (Instant::now(), Arc::new(body.to_string())));
    }
}

/// Sanitize a key fragment for use in a filename: no path separators, no
/// traversal. Mirrors the pattern already used for the changelog/hold caches
/// in intel/release.rs.
fn sanitize(s: &str) -> String {
    s.replace(['/', '@', '\\'], "_").replace("..", "_")
}

fn disk_path(eco: &str, pkg: &str, cache_dir: &Path) -> PathBuf {
    cache_dir.join(format!("regdoc_{}_{}.json", sanitize(eco), sanitize(pkg)))
}

/// Write via a sibling temp file plus rename, atomic on the same filesystem
/// (same pattern as store.rs's write_json: readers always see a complete old
/// or new file, never a partial write from a crash mid-write).
fn write_disk(path: &Path, body: &str) {
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// The registry document URL for (eco, pkg), or None when the ecosystem has
/// no registry document endpoint this cache knows how to fetch.
fn url_for(eco: &str, pkg: &str) -> Option<String> {
    match eco {
        "npm" | "npx" => Some(format!(
            "https://registry.npmjs.org/{}",
            crate::http::encode(pkg)
        )),
        "pip" => Some(format!(
            "https://pypi.org/pypi/{}/json",
            crate::http::encode(pkg)
        )),
        // crates.io's JSON API doc (a single fetch covers name/version/
        // description/downloads), the same shape every other eco's doc cache
        // entry follows. The shared http.rs agent's "napm" user-agent clears
        // crates.io's block on default/empty user agents.
        "cargo" => Some(format!(
            "https://crates.io/api/v1/crates/{}",
            crate::http::encode(pkg)
        )),
        _ => None,
    }
}

/// The registry document for (eco, pkg), from cache or network. None when the
/// fetch fails and no cached copy exists. eco: "npm"/"npx" -> packument,
/// "pip" -> PyPI JSON. Other ecosystems: None.
pub fn doc(eco: &str, pkg: &str, cache_dir: &Path) -> Option<String> {
    doc_with(crate::http::get, eco, pkg, cache_dir)
}

/// `doc` with an injectable fetch function, so the cache/fallback logic is
/// unit-testable without touching the network.
fn doc_with(
    fetch: impl Fn(&str) -> Result<String, String>,
    eco: &str,
    pkg: &str,
    cache_dir: &Path,
) -> Option<String> {
    let url = url_for(eco, pkg)?;
    let key: DocKey = (eco.to_string(), pkg.to_string());

    // 1. Memory layer.
    if let Some(body) = memory_get(&key) {
        return Some(body);
    }

    // 2. Disk layer, freshness by mtime.
    let path = disk_path(eco, pkg, cache_dir);
    if let Ok(meta) = std::fs::metadata(&path) {
        if let Ok(modified) = meta.modified() {
            if modified.elapsed().map(|e| e < TTL).unwrap_or(false) {
                if let Ok(body) = std::fs::read_to_string(&path) {
                    memory_put(&key, &body);
                    return Some(body);
                }
            }
        }
    }

    // 3. Network, with a stale-disk fallback on failure.
    match fetch(&url) {
        Ok(body) => {
            write_disk(&path, &body);
            memory_put(&key, &body);
            Some(body)
        }
        Err(_) => std::fs::read_to_string(&path).ok().inspect(|body| {
            memory_put(&key, body);
        }),
    }
}

/// Split `n` indices `[0, n)` into up to `max_workers` contiguous, balanced
/// chunks. Every index in `[0, n)` appears in exactly one chunk. Used to
/// bound thread fan-out (verdict fetches, npx_latest lookups) at a fixed
/// worker count instead of one thread per item.
pub fn chunk_indices(n: usize, max_workers: usize) -> Vec<Vec<usize>> {
    if n == 0 || max_workers == 0 {
        return Vec::new();
    }
    let workers = max_workers.min(n);
    let base = n / workers;
    let rem = n % workers;
    let mut out = Vec::with_capacity(workers);
    let mut start = 0;
    for w in 0..workers {
        let size = base + if w < rem { 1 } else { 0 };
        let end = start + size;
        out.push((start..end).collect());
        start = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn disk_hit_never_calls_fetch() {
        let dir =
            std::env::temp_dir().join(format!("napm_regdoc_test_disk_hit_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = disk_path("npm", "disk-hit-pkg", &dir);
        std::fs::write(&path, r#"{"ok":true}"#).unwrap();

        let result = doc_with(
            |_url| panic!("fetch must not be called on a fresh disk hit"),
            "npm",
            "disk-hit-pkg",
            &dir,
        );
        assert_eq!(result.as_deref(), Some(r#"{"ok":true}"#));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_disk_falls_back_when_fetch_fails() {
        let dir =
            std::env::temp_dir().join(format!("napm_regdoc_test_stale_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = disk_path("npm", "stale-pkg", &dir);
        std::fs::write(&path, "old-content").unwrap();
        // Backdate the file's mtime past the TTL.
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_modified(std::time::SystemTime::now() - Duration::from_secs(7200))
            .unwrap();

        let result = doc_with(
            |_url| Err("network down".to_string()),
            "npm",
            "stale-pkg",
            &dir,
        );
        assert_eq!(result.as_deref(), Some("old-content"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn miss_with_failing_fetch_and_no_file_is_none() {
        let dir =
            std::env::temp_dir().join(format!("napm_regdoc_test_miss_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        let result = doc_with(
            |_url| Err("network down".to_string()),
            "npm",
            "never-seen-pkg",
            &dir,
        );
        assert_eq!(result, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_sanitization_is_path_safe() {
        let dir =
            std::env::temp_dir().join(format!("napm_regdoc_test_sanitize_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        let result = doc_with(
            |_url| Ok(r#"{"scoped":true}"#.to_string()),
            "npm",
            "@scope/pkg-name",
            &dir,
        );
        assert_eq!(result.as_deref(), Some(r#"{"scoped":true}"#));
        let path = disk_path("npm", "@scope/pkg-name", &dir);
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(!name.contains('/'), "filename must not contain '/': {name}");
        assert!(path.exists(), "expected the sanitized path to be written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cargo_doc_is_fetched_and_cached_like_other_ecosystems() {
        let dir =
            std::env::temp_dir().join(format!("napm_regdoc_test_cargo_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        let result = doc_with(
            |url| {
                assert!(url.contains("crates.io/api/v1/crates/ripgrep"));
                Ok(r#"{"crate":{"max_version":"14.1.1"}}"#.to_string())
            },
            "cargo",
            "ripgrep",
            &dir,
        );
        assert_eq!(
            result.as_deref(),
            Some(r#"{"crate":{"max_version":"14.1.1"}}"#)
        );
        let path = disk_path("cargo", "ripgrep", &dir);
        assert!(path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chunk_indices_covers_every_index_exactly_once() {
        for n in [0usize, 1, 2, 7, 8, 9, 40, 100] {
            let chunks = chunk_indices(n, 8);
            assert!(chunks.len() <= 8, "n={n} produced {} chunks", chunks.len());
            let mut seen = BTreeSet::new();
            let mut total = 0;
            for c in &chunks {
                total += c.len();
                for &i in c {
                    assert!(i < n, "index {i} out of range for n={n}");
                    assert!(seen.insert(i), "index {i} duplicated for n={n}");
                }
            }
            assert_eq!(total, n);
            assert_eq!(seen.len(), n);
        }
    }
}
