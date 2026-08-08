//! Offline install-size helpers. The "Size" column shows the on-disk footprint
//! of each installed package, computed by walking its files (npm/brew/npx) or
//! summing the pip dist-info RECORD.

use std::path::Path;

/// Format a byte count like the prototype: integer MB above 1 MB, KB below,
/// one decimal GB above 1 GB. 0 bytes renders empty (no size known).
pub fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes == 0 {
        String::new()
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{} MB", (bytes + MB / 2) / MB)
    } else if bytes >= KB {
        format!("{} KB", (bytes + KB / 2) / KB)
    } else {
        format!("{} B", bytes)
    }
}

/// Recursively sum the sizes of all regular files under `path`. Symlinks are
/// skipped (no following) to avoid loops and double-counting. Returns 0 if the
/// path does not exist.
pub fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    for entry in entries.flatten() {
        let ft = match entry.file_type() {
            Ok(f) => f,
            Err(_) => continue,
        };
        if ft.is_symlink() {
            continue;
        } else if ft.is_dir() {
            total += dir_size(&entry.path());
        } else if ft.is_file() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// Sum the byte sizes from a pip dist-info RECORD file. Each line is
/// `path,hash,size`; the trailing size field is summed when it is numeric
/// (some lines, like the RECORD entry itself, have no size).
pub fn record_total_size(record: &str) -> u64 {
    record
        .lines()
        .filter_map(|line| {
            line.rsplit(',')
                .next()
                .and_then(|s| s.trim().parse::<u64>().ok())
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_formats_by_magnitude() {
        assert_eq!(human_size(0), "");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1 KB");
        assert_eq!(human_size(38 * 1024 * 1024), "38 MB");
        // rounds to nearest MB
        assert_eq!(human_size(1024 * 1024 + 1024 * 1024 / 2), "2 MB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn record_total_size_sums_numeric_sizes() {
        let record = "foo/bar.py,sha256=abc,100\nbaz.py,sha256=def,250\nRECORD,,\n";
        assert_eq!(record_total_size(record), 350);
        assert_eq!(record_total_size(""), 0);
    }
}
