use super::InstalledTool;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The first version-like token in `s`: a run of digits and dots that contains
/// at least one dot and starts with a digit (e.g. "0.2.14", "1.4"). Trailing
/// dots are trimmed. Returns None when there is no such token.
/// "grok-0.2.14-macos-aarch64" -> "0.2.14"; "grok 0.2.14 (e0d895d)" -> "0.2.14".
pub fn first_version(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            let tok = s[start..i].trim_end_matches('.');
            let parts: Vec<&str> = tok.split('.').collect();
            if parts.len() >= 2
                && !parts[0].is_empty()
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
            {
                return Some(tok.to_string());
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Directory prefixes whose contents are owned by a package manager, toolchain,
/// or the OS, and must never be surfaced as manual installs.
fn managed_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
        PathBuf::from("/usr/libexec"),
        PathBuf::from("/System"),
        PathBuf::from("/Library/Apple"),
        PathBuf::from("/opt/homebrew"),
        PathBuf::from("/usr/local/Cellar"),
        PathBuf::from("/usr/local/Homebrew"),
        // TeX Live: a tlmgr-managed tree, not manual installs. Without this its
        // hundreds of bundled perl/lua scripts flood the library as "manual".
        PathBuf::from("/usr/local/texlive"),
    ];
    // Resolved Homebrew prefix, if brew is installed (covers non-standard prefixes).
    if let Some(prefix) = super::brew::brew_prefix() {
        roots.push(prefix.clone());
    }
    // Home-relative toolchain / version-manager dirs and the npx cache.
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for sub in [
            ".cargo", ".rustup", ".nvm", ".pyenv", ".volta", ".asdf", "go/bin",
        ] {
            roots.push(home.join(sub));
        }
        roots.push(home.join(".npm").join("_npx"));
    }
    // npm global modules: a global CLI on PATH resolves into
    // <prefix>/lib/node_modules, which `npm root -g` reports. Without this, npm
    // and npx globals leak in as "manual" wherever the prefix is not a brew root.
    if let Some(root) = super::npm::npm_root() {
        roots.push(root.clone());
    }
    // pip user console-script dir (e.g. ~/Library/Python/3.x/bin/<script>), the
    // macOS `pip3 install` default. The pip row name is the distribution name,
    // not the script basename, so only a path root catches these. The GLOBAL
    // scripts dir is intentionally NOT excluded: on macOS it is /usr/local/bin,
    // far too broad (a common curl|bash drop target for real manual tools).
    let (_, user_base) = super::pip::python_site();
    if let Some(ub) = user_base {
        roots.push(ub.join("bin"));
    }
    roots
}

/// What to do to determine a candidate's version, decided WITHOUT running
/// anything: a filename token, a cached probe result, or "no version" (the
/// binary is outside $HOME). Only `Probe` requires spawning a process, via
/// `probe_version` below.
#[derive(Debug, PartialEq)]
enum VersionPlan {
    Known(String),
    Probe,
}

/// Decide the version plan for a resolved binary path. Mirrors the previous
/// `resolve_version`'s precedence: a filename token first (free); then, only
/// when the binary resolves under $HOME, a cached result if one matches, else
/// a request to probe.
/// `-v` is intentionally not probed: it commonly means "verbose" and can start
/// a real or long-running process on an unknown binary.
fn plan_version(real: &Path, home: Option<&Path>, cached: Option<&str>) -> VersionPlan {
    if let Some(name) = real.file_name().and_then(|n| n.to_str()) {
        if let Some(v) = first_version(name) {
            return VersionPlan::Known(v);
        }
    }
    let under_home = match home {
        Some(h) => real.starts_with(h),
        None => false,
    };
    if !under_home {
        return VersionPlan::Known(String::new());
    }
    if let Some(v) = cached {
        return VersionPlan::Known(v.to_string());
    }
    VersionPlan::Probe
}

/// Execute the actual probe for a `VersionPlan::Probe` candidate: `<tool>
/// --version` then `<tool> version`, each with a 2-second kill timer. Empty
/// when neither answers with a recognizable version.
fn probe_version(real: &Path) -> String {
    for arg in ["--version", "version"] {
        if let Some(out) = run_with_timeout(real, arg, Duration::from_millis(2000)) {
            if let Some(v) = first_version(&out) {
                return v;
            }
        }
    }
    String::new()
}

/// Run `bin arg`, capturing stdout+stderr, killing it if it exceeds `dur`.
/// Returns the combined output, or None on spawn failure or timeout.
fn run_with_timeout(bin: &Path, arg: &str, dur: Duration) -> Option<String> {
    let mut child = Command::new(bin)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() >= dur {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(s)
}

/// One cached probe result, keyed by the resolved binary's path. A hit
/// requires both `mtime` and `size` to match the binary's current stat, so a
/// replaced or upgraded binary is re-probed. `version` may be "" to record a
/// FAILED probe (neither flag answered), so a version-less binary is not
/// re-executed on every scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProbeEntry {
    mtime: i64,
    size: u64,
    version: String,
}

/// The on-disk shape of `manual_probe.json`: resolved path -> cached result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProbeCache {
    #[serde(flatten)]
    entries: HashMap<String, ProbeEntry>,
}

fn probe_cache_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("manual_probe.json")
}

/// Load the probe cache. A missing or corrupt file degrades to an empty cache
/// rather than panicking or failing the scan.
fn load_probe_cache(cache_dir: &Path) -> ProbeCache {
    std::fs::read_to_string(probe_cache_path(cache_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Write the probe cache via a sibling temp file plus rename (atomic on the
/// same filesystem), owner-only mode on unix. Mirrors `Store::write_json`.
fn save_probe_cache(cache_dir: &Path, cache: &ProbeCache) {
    if let Ok(s) = serde_json::to_string_pretty(cache) {
        let path = probe_cache_path(cache_dir);
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            #[cfg(unix)]
            {
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            }
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// At most this many probe subprocesses run at once. A cold cache with a
/// handful of unmanaged $HOME binaries stays fast; it never fans out to
/// dozens of simultaneous child processes.
const MAX_PROBE_WORKERS: usize = 4;

/// A binary found on $PATH that no package manager owns, before its version
/// is known.
struct Candidate {
    real: PathBuf,
    basename: String,
    mtime: i64,
    size: u64,
}

/// Scan every $PATH directory for executables that no package manager owns,
/// resolving symlinks, excluding managed/app/toolchain/system paths and names
/// already returned by the other scanners, deduped by resolved target.
/// `other_names` is the set of tool names from the npm/brew/pip/npx scans.
/// `cache_dir` is where `manual_probe.json` is read from and written to.
pub fn scan_manual(other_names: &BTreeSet<String>, cache_dir: &Path) -> Vec<InstalledTool> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let roots = managed_roots();
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return Vec::new(),
    };

    // Phase 1: walk $PATH and collect candidates. Filesystem-only: nothing is
    // executed here.
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for dir in std::env::split_paths(&path) {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let candidate = entry.path();
            // Must be executable (regular file or symlink to one).
            let meta = match std::fs::metadata(&candidate) {
                Ok(m) => m,
                Err(_) => continue, // broken symlink, etc.
            };
            if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
                continue;
            }
            let real = match std::fs::canonicalize(&candidate) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let basename = match candidate.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if is_managed(&real, &basename, &roots, other_names) {
                continue;
            }
            if !seen.insert(real.clone()) {
                continue; // already processed this target
            }
            let mtime = super::path_mtime(&real);
            candidates.push(Candidate {
                real,
                basename,
                mtime,
                size: meta.len(),
            });
        }
    }

    // Phase 2: decide each candidate's version plan against the cache, then
    // resolve the cache-miss subset on a bounded worker pool.
    let mut cache = load_probe_cache(cache_dir);

    let mut plans: Vec<VersionPlan> = Vec::with_capacity(candidates.len());
    let mut miss_indices: Vec<usize> = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        let key = c.real.to_string_lossy().into_owned();
        let cached = cache
            .entries
            .get(&key)
            .filter(|e| e.mtime == c.mtime && e.size == c.size)
            .map(|e| e.version.as_str());
        let plan = plan_version(&c.real, home.as_deref(), cached);
        if plan == VersionPlan::Probe {
            miss_indices.push(i);
        }
        plans.push(plan);
    }

    if !miss_indices.is_empty() {
        let chunk_size = miss_indices.len().div_ceil(MAX_PROBE_WORKERS).max(1);
        let results: Mutex<Vec<(usize, String)>> = Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for chunk in miss_indices.chunks(chunk_size) {
                let candidates_ref = &candidates;
                let results_ref = &results;
                s.spawn(move || {
                    let mut local = Vec::with_capacity(chunk.len());
                    for &idx in chunk {
                        let v = probe_version(&candidates_ref[idx].real);
                        local.push((idx, v));
                    }
                    results_ref.lock().unwrap().extend(local);
                });
            }
        });
        for (idx, version) in results.into_inner().unwrap() {
            let c = &candidates[idx];
            cache.entries.insert(
                c.real.to_string_lossy().into_owned(),
                ProbeEntry {
                    mtime: c.mtime,
                    size: c.size,
                    version: version.clone(),
                },
            );
            plans[idx] = VersionPlan::Known(version);
        }
        save_probe_cache(cache_dir, &cache);
    }

    // Phase 3: assemble rows in candidate (original walk) order.
    let mut rows: Vec<InstalledTool> = Vec::with_capacity(candidates.len());
    for (c, plan) in candidates.into_iter().zip(plans) {
        let version = match plan {
            VersionPlan::Known(v) => v,
            VersionPlan::Probe => String::new(), // unreachable: all misses resolved above
        };
        let size = super::size::human_size(super::size::dir_size(&c.real));
        rows.push(InstalledTool {
            name: c.basename.clone(),
            eco: "manual".to_string(),
            pkg: c.basename,
            installed: Some(version.clone()),
            latest: version,
            size,
            pinned: false,
            publisher: "local".to_string(),
            description: c.real.to_string_lossy().into_owned(),
            updated: c.mtime,
            requested: true,
            status: String::new(),
            bump: String::new(),
        });
    }
    dedup_by_target(rows)
}

/// Collapse rows sharing a resolved target path (stored in `description`),
/// keeping the first seen. Sorted by display name for stable output.
pub fn dedup_by_target(rows: Vec<InstalledTool>) -> Vec<InstalledTool> {
    let mut map: BTreeMap<String, InstalledTool> = BTreeMap::new();
    for row in rows {
        map.entry(row.description.clone()).or_insert(row);
    }
    let mut out: Vec<InstalledTool> = map.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// True when `real` (a fully-resolved path) belongs to something napm must not
/// claim as a manual install: an app bundle, any managed root prefix, or a
/// basename already returned by the npm/brew/pip/npx scans.
pub fn is_managed(
    real: &Path,
    basename: &str,
    managed_roots: &[PathBuf],
    other_names: &BTreeSet<String>,
) -> bool {
    if other_names.contains(basename) {
        return true;
    }
    if real.to_string_lossy().contains(".app/") {
        return true;
    }
    managed_roots.iter().any(|root| real.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manual_row(name: &str, target: &str, ver: &str) -> InstalledTool {
        InstalledTool {
            name: name.to_string(),
            eco: "manual".to_string(),
            pkg: name.to_string(),
            installed: Some(ver.to_string()),
            latest: ver.to_string(),
            size: String::new(),
            pinned: false,
            publisher: "local".to_string(),
            description: target.to_string(),
            updated: 0,
            requested: true,
            status: String::new(),
            bump: String::new(),
        }
    }

    #[test]
    fn dedup_collapses_same_target() {
        let rows = dedup_by_target(vec![
            manual_row("grok", "/Users/x/.grok/downloads/grok-0.2.14", "0.2.14"),
            manual_row("grok", "/Users/x/.grok/downloads/grok-0.2.14", "0.2.14"),
            manual_row("agent", "/Users/x/.grok/downloads/agent-bin", "0.2.14"),
        ]);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.name == "grok"));
        assert!(rows.iter().any(|r| r.name == "agent"));
    }

    #[test]
    fn excludes_managed_paths_and_known_names() {
        let mut roots: Vec<PathBuf> = vec![
            PathBuf::from("/opt/homebrew"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/Users/x/.cargo"),
        ];
        roots.sort();
        let mut names = BTreeSet::new();
        names.insert("eslint".to_string());

        // Homebrew cellar (under /opt/homebrew)
        assert!(is_managed(
            Path::new("/opt/homebrew/Cellar/foo/1.0/bin/foo"),
            "foo",
            &roots,
            &names
        ));
        // app bundle CLI
        assert!(is_managed(
            Path::new("/Applications/Docker.app/Contents/Resources/bin/docker"),
            "docker",
            &roots,
            &names
        ));
        // cargo toolchain
        assert!(is_managed(
            Path::new("/Users/x/.cargo/bin/cargo"),
            "cargo",
            &roots,
            &names
        ));
        // system dir
        assert!(is_managed(Path::new("/usr/bin/ls"), "ls", &roots, &names));
        // name already owned by npm/pip/npx/brew scan, regardless of path
        assert!(is_managed(
            Path::new("/Users/x/.local/bin/eslint"),
            "eslint",
            &roots,
            &names
        ));
        // a genuinely-manual tool: not excluded
        assert!(!is_managed(
            Path::new("/Users/x/.local/bin/agy"),
            "agy",
            &roots,
            &names
        ));
    }

    #[test]
    fn version_from_filename() {
        assert_eq!(
            first_version("grok-0.2.14-macos-aarch64").as_deref(),
            Some("0.2.14")
        );
        assert_eq!(first_version("tool-v1.2").as_deref(), Some("1.2"));
        assert_eq!(first_version("agy").as_deref(), None);
        assert_eq!(first_version("aarch64").as_deref(), None); // digits, no dot
    }

    #[test]
    fn version_from_output() {
        assert_eq!(
            first_version("grok 0.2.14 (e0d895d)").as_deref(),
            Some("0.2.14")
        );
        assert_eq!(first_version("v1.4.0").as_deref(), Some("1.4.0"));
        assert_eq!(
            first_version("some build, no version here").as_deref(),
            None
        );
    }

    fn temp_cache_dir(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("napm-manual-probe-test-{}-{:p}", tag, &dir));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn probe_cache_roundtrip_hits_on_matching_mtime_and_size() {
        let dir = temp_cache_dir("roundtrip");
        let mut cache = ProbeCache::default();
        cache.entries.insert(
            "/Users/x/.local/bin/agy".to_string(),
            ProbeEntry {
                mtime: 1000,
                size: 4096,
                version: "1.2.3".to_string(),
            },
        );
        save_probe_cache(&dir, &cache);

        let loaded = load_probe_cache(&dir);
        let entry = loaded.entries.get("/Users/x/.local/bin/agy").unwrap();
        assert_eq!(entry.mtime, 1000);
        assert_eq!(entry.size, 4096);
        assert_eq!(entry.version, "1.2.3");

        // Hit: mtime+size match.
        let cached = loaded
            .entries
            .get("/Users/x/.local/bin/agy")
            .filter(|e| e.mtime == 1000 && e.size == 4096)
            .map(|e| e.version.as_str());
        assert_eq!(cached, Some("1.2.3"));

        // Miss: mtime changed (binary replaced).
        let cached_after_change = loaded
            .entries
            .get("/Users/x/.local/bin/agy")
            .filter(|e| e.mtime == 1001 && e.size == 4096)
            .map(|e| e.version.as_str());
        assert_eq!(cached_after_change, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_probe_caches_as_empty_and_is_a_hit() {
        let dir = temp_cache_dir("failed");
        let mut cache = ProbeCache::default();
        cache.entries.insert(
            "/Users/x/.local/bin/mystery".to_string(),
            ProbeEntry {
                mtime: 42,
                size: 8,
                version: String::new(),
            },
        );
        save_probe_cache(&dir, &cache);

        let loaded = load_probe_cache(&dir);
        let cached = loaded
            .entries
            .get("/Users/x/.local/bin/mystery")
            .filter(|e| e.mtime == 42 && e.size == 8)
            .map(|e| e.version.as_str());
        // A hit, not a miss: the recorded failure ("") is itself the cached answer.
        assert_eq!(cached, Some(""));

        // plan_version must treat this as Known(""), not Probe.
        let plan = plan_version(Path::new("/Users/x/.local/bin/mystery"), None, cached);
        assert_eq!(plan, VersionPlan::Known(String::new()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_probe_cache_degrades_to_empty_no_panic() {
        let dir = temp_cache_dir("corrupt");
        std::fs::write(probe_cache_path(&dir), b"not json").unwrap();
        let loaded = load_probe_cache(&dir);
        assert!(loaded.entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_probe_cache_degrades_to_empty_no_panic() {
        let dir = temp_cache_dir("missing");
        let loaded = load_probe_cache(&dir);
        assert!(loaded.entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plan_outside_home_never_requests_a_probe() {
        let home = Path::new("/Users/x");
        let real = Path::new("/opt/local/bin/mystery");
        let plan = plan_version(real, Some(home), None);
        assert_eq!(plan, VersionPlan::Known(String::new()));
    }

    #[test]
    fn plan_under_home_with_no_cache_requests_a_probe() {
        let home = Path::new("/Users/x");
        let real = Path::new("/Users/x/.local/bin/mystery");
        let plan = plan_version(real, Some(home), None);
        assert_eq!(plan, VersionPlan::Probe);
    }

    #[test]
    fn plan_under_home_with_matching_cache_is_known() {
        let home = Path::new("/Users/x");
        let real = Path::new("/Users/x/.local/bin/mystery");
        let plan = plan_version(real, Some(home), Some("1.2.3"));
        assert_eq!(plan, VersionPlan::Known("1.2.3".to_string()));
    }
}
