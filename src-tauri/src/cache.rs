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
