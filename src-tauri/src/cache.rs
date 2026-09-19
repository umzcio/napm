//! Shared helpers for the app's JSON cache files: one atomic write and one
//! filename sanitizer, so every cache layer follows the same rules instead
//! of drifting copies.
use std::path::Path;

/// Write via a sibling temp file plus rename, atomic on the same
/// filesystem: readers always see a complete old or new file, never a
/// partial write from a crash or a concurrent writer.
pub(crate) fn write_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)
}

/// Sanitize a key fragment for use in a cache filename: no path separators,
/// no traversal, no `@` (scoped npm names).
pub(crate) fn sanitize_key(s: &str) -> String {
    s.replace(['/', '@', '\\'], "_").replace("..", "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_key_replaces_path_unsafe_characters() {
        assert_eq!(sanitize_key("plain"), "plain");
        assert_eq!(sanitize_key("a/b"), "a_b");
        assert_eq!(sanitize_key("a\\b"), "a_b");
        assert_eq!(sanitize_key("@scope/pkg"), "_scope_pkg");
        assert_eq!(sanitize_key(".."), "_");
        // ".." is replaced non-overlapping left to right, so "..." -> "_.".
        assert_eq!(sanitize_key("..."), "_.");
        assert_eq!(sanitize_key("../etc"), "__etc");
    }

    #[test]
    fn write_atomic_round_trips_and_leaves_no_tmp_file() {
        let dir = std::env::temp_dir().join(format!("napm_cache_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("entry.json");

        write_atomic(&path, "hello").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        assert!(!path.with_extension("json.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
