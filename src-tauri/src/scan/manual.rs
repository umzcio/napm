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

#[cfg(test)]
mod tests {
    use super::*;

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
