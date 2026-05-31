use super::InstalledTool;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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
