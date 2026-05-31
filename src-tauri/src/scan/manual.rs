use super::InstalledTool;
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
                && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
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
    ];
    // Resolved Homebrew prefix, if brew is installed (covers non-standard prefixes).
    if let Ok(out) = Command::new("brew").arg("--prefix").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            roots.push(PathBuf::from(p));
        }
    }
    // Home-relative toolchain / version-manager dirs and the npx cache.
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for sub in [".cargo", ".rustup", ".nvm", ".pyenv", ".volta", ".asdf", "go/bin"] {
            roots.push(home.join(sub));
        }
        roots.push(home.join(".npm").join("_npx"));
    }
    // npm global modules: a global CLI on PATH resolves into
    // <prefix>/lib/node_modules, which `npm root -g` reports. Without this, npm
    // and npx globals leak in as "manual" wherever the prefix is not a brew root.
    if let Ok(out) = Command::new("npm").arg("root").arg("-g").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            roots.push(PathBuf::from(p));
        }
    }
    // pip user console-script dir (e.g. ~/Library/Python/3.x/bin/<script>), the
    // macOS `pip3 install` default. The pip row name is the distribution name,
    // not the script basename, so only a path root catches these. The GLOBAL
    // scripts dir is intentionally NOT excluded: on macOS it is /usr/local/bin,
    // far too broad (a common curl|bash drop target for real manual tools).
    let user_base = super::run("python3", &["-c", "import site; print(site.getuserbase())"]);
    let ub = user_base.trim();
    if !ub.is_empty() {
        roots.push(PathBuf::from(ub).join("bin"));
    }
    roots
}

/// Resolve a tool's version: a token in the resolved filename first (free, no
/// execution), then `<tool> --version`/`version` but ONLY when the binary
/// resolves under $HOME (never run system-wide binaries). Empty when unknown.
/// `-v` is intentionally not probed: it commonly means "verbose" and can start
/// a real or long-running process on an unknown binary.
fn resolve_version(real: &Path, home: Option<&Path>) -> String {
    if let Some(name) = real.file_name().and_then(|n| n.to_str()) {
        if let Some(v) = first_version(name) {
            return v;
        }
    }
    let under_home = match home {
        Some(h) => real.starts_with(h),
        None => false,
    };
    if under_home {
        for arg in ["--version", "version"] {
            if let Some(out) = run_with_timeout(real, arg, Duration::from_millis(2000)) {
                if let Some(v) = first_version(&out) {
                    return v;
                }
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

/// Scan every $PATH directory for executables that no package manager owns,
/// resolving symlinks, excluding managed/app/toolchain/system paths and names
/// already returned by the other scanners, deduped by resolved target.
/// `other_names` is the set of tool names from the npm/brew/pip/npx scans.
pub fn scan_manual(other_names: &BTreeSet<String>) -> Vec<InstalledTool> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let roots = managed_roots();
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return Vec::new(),
    };

    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut rows: Vec<InstalledTool> = Vec::new();

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
            let version = resolve_version(&real, home.as_deref());
            let size = super::size::human_size(super::size::dir_size(&real));
            rows.push(InstalledTool {
                name: basename.clone(),
                eco: "manual".to_string(),
                pkg: basename,
                installed: Some(version.clone()),
                latest: version,
                size,
                pinned: false,
                publisher: "local".to_string(),
                description: real.to_string_lossy().into_owned(),
                updated: super::path_mtime(&real),
                requested: true,
            });
        }
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
        assert!(is_managed(Path::new("/opt/homebrew/Cellar/foo/1.0/bin/foo"), "foo", &roots, &names));
        // app bundle CLI
        assert!(is_managed(Path::new("/Applications/Docker.app/Contents/Resources/bin/docker"), "docker", &roots, &names));
        // cargo toolchain
        assert!(is_managed(Path::new("/Users/x/.cargo/bin/cargo"), "cargo", &roots, &names));
        // system dir
        assert!(is_managed(Path::new("/usr/bin/ls"), "ls", &roots, &names));
        // name already owned by npm/pip/npx/brew scan, regardless of path
        assert!(is_managed(Path::new("/Users/x/.local/bin/eslint"), "eslint", &roots, &names));
        // a genuinely-manual tool: not excluded
        assert!(!is_managed(Path::new("/Users/x/.local/bin/agy"), "agy", &roots, &names));
    }

    #[test]
    fn version_from_filename() {
        assert_eq!(first_version("grok-0.2.14-macos-aarch64").as_deref(), Some("0.2.14"));
        assert_eq!(first_version("tool-v1.2").as_deref(), Some("1.2"));
        assert_eq!(first_version("agy").as_deref(), None);
        assert_eq!(first_version("aarch64").as_deref(), None); // digits, no dot
    }

    #[test]
    fn version_from_output() {
        assert_eq!(first_version("grok 0.2.14 (e0d895d)").as_deref(), Some("0.2.14"));
        assert_eq!(first_version("v1.4.0").as_deref(), Some("1.4.0"));
        assert_eq!(first_version("some build, no version here").as_deref(), None);
    }
}
