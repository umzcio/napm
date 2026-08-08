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

/// Status for a Shared Library row: "unmanaged" (eco == "manual", no package
/// manager owns it), "offline" (not installed), "current", or "update".
/// Uses `cmp()`, so a `latest` that is not genuinely newer than `installed`
/// (equal, a downgrade, or only different by prerelease/suffix noise) never
/// reads as "update" -- a private or scope-shadowed package that reports a
/// lower public "latest" must not show as an available update.
pub fn status_of(eco: &str, installed: Option<&str>, latest: &str) -> &'static str {
    if eco == "manual" {
        return "unmanaged";
    }
    let installed = match installed {
        Some(v) if !v.is_empty() => v,
        _ => return "offline",
    };
    if cmp(latest, installed) == Ordering::Greater {
        "update"
    } else {
        "current"
    }
}

/// "major" | "minor" | "patch" | "none" from the first differing numeric
/// segment of `installed` vs `latest` (segments 0/1/2 map to major/minor/
/// patch). A difference confined to the 4th+ segment, or to a prerelease/
/// suffix on an otherwise-equal numeric triple, is classified "patch" -- it
/// is a genuine `cmp()` change too small to be major or minor, and "none"
/// is reserved for "no version step at all" (empty input or equal versions).
pub fn bump_kind(installed: &str, latest: &str) -> &'static str {
    if installed.is_empty() || latest.is_empty() {
        return "none";
    }
    if cmp(installed, latest) == Ordering::Equal {
        return "none";
    }
    let a = version_triple(installed);
    let b = version_triple(latest);
    if a.0 != b.0 {
        "major"
    } else if a.1 != b.1 {
        "minor"
    } else {
        "patch"
    }
}

/// First three numeric segments (major, minor, patch), ignoring any
/// non-numeric suffix. Missing segments count as 0.
fn version_triple(v: &str) -> (u64, u64, u64) {
    let parts: Vec<&str> = v.split('.').collect();
    let seg = |i: usize| -> u64 { parts.get(i).map(|s| split_numeric_prefix(s).0).unwrap_or(0) };
    (seg(0), seg(1), seg(2))
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

    /// Characterization table for status_of/bump_kind: normal semver triples
    /// (must not regress from the old JS behavior) plus the blind spots the
    /// old JS `verParts` truncation/coercion got wrong (prerelease compared
    /// equal to release, 4-segment pip versions compared equal on their
    /// first three, downgrades flagged as updates).
    #[test]
    fn status_of_and_bump_kind_table() {
        struct Case {
            eco: &'static str,
            installed: Option<&'static str>,
            latest: &'static str,
            status: &'static str,
            bump: Option<&'static str>,
        }
        let cases = [
            Case {
                eco: "npm",
                installed: Some("1.0.0"),
                latest: "1.0.0",
                status: "current",
                bump: Some("none"),
            },
            Case {
                eco: "npm",
                installed: Some("1.0.0"),
                latest: "1.0.1",
                status: "update",
                bump: Some("patch"),
            },
            Case {
                eco: "npm",
                installed: Some("1.0.0"),
                latest: "1.2.0",
                status: "update",
                bump: Some("minor"),
            },
            Case {
                eco: "npm",
                installed: Some("1.0.0"),
                latest: "2.0.0",
                status: "update",
                bump: Some("major"),
            },
            Case {
                eco: "npm",
                installed: None,
                latest: "1.0.0",
                status: "offline",
                bump: None,
            },
            Case {
                eco: "manual",
                installed: Some("1.0.0"),
                latest: "1.0.0",
                status: "unmanaged",
                bump: None,
            },
            Case {
                eco: "npm",
                installed: Some("2.0.0"),
                latest: "1.9.0",
                status: "current", // downgrade is never an update
                bump: None,
            },
            Case {
                eco: "pip",
                installed: Some("1.2.3.5"),
                latest: "1.2.3.4",
                status: "current", // 4-segment blind spot fixed
                bump: None,
            },
            Case {
                eco: "pip",
                installed: Some("1.2.3.4"),
                latest: "1.2.3.5",
                status: "update",
                bump: Some("patch"), // 4th-segment bump counts as patch
            },
            Case {
                eco: "npm",
                installed: Some("2.0.0"),
                latest: "2.0.0-rc.1",
                status: "current", // prerelease of the same version is not an upgrade
                bump: None,
            },
            Case {
                eco: "npm",
                installed: Some("2.0.0-rc.1"),
                latest: "2.0.0",
                status: "update",
                bump: Some("patch"), // same numeric triple: classified as patch
            },
        ];
        for c in cases {
            assert_eq!(
                status_of(c.eco, c.installed, c.latest),
                c.status,
                "status_of({:?}, {:?}, {:?})",
                c.eco,
                c.installed,
                c.latest
            );
            if let Some(expected_bump) = c.bump {
                assert_eq!(
                    bump_kind(c.installed.unwrap_or(""), c.latest),
                    expected_bump,
                    "bump_kind({:?}, {:?})",
                    c.installed,
                    c.latest
                );
            }
        }
    }
}
