//! Numeric version comparison. Not full semver; just strict enough that napm's
//! dedup and drift logic never falls back to lexicographic string ordering
//! (where "1.9.0" > "1.10.0" is true, which is wrong).

use std::cmp::Ordering;

/// Compare two version strings numerically. Split on '.', compare numeric
/// prefixes of each segment as integers; a segment with a non-numeric suffix
/// (prerelease like "0-rc1" or brew revision "3_1") sorts BELOW the same
/// numeric value without a suffix. Missing segments count as 0. Not full
/// semver; just strict enough that 1.10.0 > 1.9.0 and 1.0.0-rc1 < 1.0.0.
pub fn cmp(a: &str, b: &str) -> Ordering {
    let a_parts: Vec<&str> = a.split('.').collect();
    let b_parts: Vec<&str> = b.split('.').collect();
    let len = a_parts.len().max(b_parts.len());
    for i in 0..len {
        let a_seg = a_parts.get(i).copied().unwrap_or("");
        let b_seg = b_parts.get(i).copied().unwrap_or("");
        let (a_num, a_rest) = split_numeric_prefix(a_seg);
        let (b_num, b_rest) = split_numeric_prefix(b_seg);
        match a_num.cmp(&b_num) {
            Ordering::Equal => {}
            other => return other,
        }
        // A segment with no suffix (e.g. "0") beats the same number with a
        // suffix (e.g. "0-rc1" or "3_1"): empty remainder sorts above non-empty.
        match (a_rest.is_empty(), b_rest.is_empty()) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        match a_rest.cmp(b_rest) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

/// Split a segment into its leading digit run (parsed as u64, 0 if none) and
/// the remainder string.
fn split_numeric_prefix(seg: &str) -> (u64, &str) {
    let end = seg
        .as_bytes()
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    let num = seg[..end].parse::<u64>().unwrap_or(0);
    (num, &seg[end..])
}

/// Ordering over Option<&str>: None sorts below any Some.
pub fn cmp_opt(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(a), Some(b)) => cmp(a, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_beats_lexicographic() {
        assert_eq!(cmp("1.10.0", "1.9.0"), Ordering::Greater);
        assert_eq!(cmp("1.2.0", "1.10.0"), Ordering::Less);
    }

    #[test]
    fn equal_versions() {
        assert_eq!(cmp("1.0.0", "1.0.0"), Ordering::Equal);
        assert_eq!(cmp("2.0", "2.0.0"), Ordering::Equal);
    }

    #[test]
    fn prerelease_sorts_below_release() {
        assert_eq!(cmp("1.0.0-rc1", "1.0.0"), Ordering::Less);
    }

    #[test]
    fn brew_revision_suffix_sorts_below_bare() {
        assert_eq!(cmp("1.2.3_1", "1.2.3"), Ordering::Less);
    }

    #[test]
    fn cmp_opt_none_is_less_than_some() {
        assert_eq!(cmp_opt(None, Some("0.0.1")), Ordering::Less);
        assert_eq!(cmp_opt(Some("0.0.1"), None), Ordering::Greater);
        assert_eq!(cmp_opt(None, None), Ordering::Equal);
    }
}
