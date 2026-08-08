use super::SearchResult;
use serde_json::Value;

/// Parse a crates.io crate JSON (`GET /api/v1/crates/<name>`) into a single
/// result. None on parse failure or a missing crate object (a 404 body will
/// not parse to a usable object).
pub fn parse_crate(json: &str) -> Option<SearchResult> {
    let v: Value = serde_json::from_str(json).ok()?;
    let c = v.get("crate")?;
    let name = c.get("name").and_then(|x| x.as_str())?;
    if name.is_empty() {
        return None;
    }
    let version = c
        .get("max_stable_version")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| c.get("max_version").and_then(|x| x.as_str()))
        .unwrap_or("")
        .to_string();
    let description = c
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    Some(SearchResult {
        name: name.to_string(),
        eco: "cargo".into(),
        pkg: name.to_string(),
        version,
        weekly_downloads: 0,
        size: String::new(),
        description,
    })
}

/// Sum the last 7 distinct days of downloads (across every version) from a
/// crates.io `/api/v1/crates/<name>/downloads` response, which returns
/// per-version-per-day counts for roughly the last 90 days. A real weekly
/// figure, not an all-time or 90-day total labeled as weekly.
pub fn parse_cargo_weekly_downloads(json: &str) -> u64 {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let entries = match v.get("version_downloads").and_then(|a| a.as_array()) {
        Some(a) => a,
        None => return 0,
    };
    let mut dates: Vec<&str> = entries
        .iter()
        .filter_map(|e| e.get("date").and_then(|d| d.as_str()))
        .collect();
    dates.sort_unstable();
    dates.dedup();
    let recent: std::collections::BTreeSet<&str> = dates.into_iter().rev().take(7).collect();
    entries
        .iter()
        .filter(|e| {
            e.get("date")
                .and_then(|d| d.as_str())
                .map(|d| recent.contains(d))
                .unwrap_or(false)
        })
        .filter_map(|e| e.get("downloads").and_then(|d| d.as_u64()))
        .sum()
}

/// Exact-name crates.io lookup. Not fuzzy: crates.io has no fuzzy search
/// endpoint this app integrates (npm's is federated instead); the cargo
/// source label in the UI explains the exact-match limitation, mirroring pip.
pub fn search_cargo(query: &str) -> Vec<SearchResult> {
    let body = match crate::http::get(&format!(
        "https://crates.io/api/v1/crates/{}",
        crate::http::encode(query)
    )) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let mut r = match parse_crate(&body) {
        Some(r) => r,
        None => return Vec::new(),
    };
    let dl_url = format!(
        "https://crates.io/api/v1/crates/{}/downloads",
        crate::http::encode(&r.pkg)
    );
    if let Ok(dl_body) = crate::http::get(&dl_url) {
        r.weekly_downloads = parse_cargo_weekly_downloads(&dl_body);
    }
    vec![r]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_crate_info() {
        let json = r#"{"crate":{"name":"ripgrep","max_stable_version":"14.1.1","max_version":"14.1.1","description":"line-oriented search tool"}}"#;
        let r = parse_crate(json).unwrap();
        assert_eq!(r.pkg, "ripgrep");
        assert_eq!(r.eco, "cargo");
        assert_eq!(r.version, "14.1.1");
        assert_eq!(r.description, "line-oriented search tool");
    }

    #[test]
    fn falls_back_to_max_version_when_no_stable() {
        let json = r#"{"crate":{"name":"pre-release-crate","max_version":"0.1.0-alpha"}}"#;
        let r = parse_crate(json).unwrap();
        assert_eq!(r.version, "0.1.0-alpha");
    }

    #[test]
    fn miss_or_garbage_is_none() {
        assert!(parse_crate(r#"{"errors":[{"detail":"Not Found"}]}"#).is_none());
        assert!(parse_crate("nope").is_none());
    }

    #[test]
    fn sums_last_seven_distinct_days_across_versions() {
        let json = r#"{"version_downloads":[
            {"version":1,"downloads":100,"date":"2024-01-01"},
            {"version":2,"downloads":50,"date":"2024-01-01"},
            {"version":1,"downloads":10,"date":"2024-01-02"},
            {"version":1,"downloads":999,"date":"2023-01-01"}
        ]}"#;
        // 7-day window here is just the 3 dates present (fewer than 7 exist),
        // but 2023-01-01 (older, and outside a real 7-day window once more
        // dates are present) still gets summed when it's within the most
        // recent 7 distinct dates -- with only 3 distinct dates total, all 3
        // are "the most recent 7". This exercises the multi-version-per-day
        // summation and dedup logic, not window truncation.
        assert_eq!(parse_cargo_weekly_downloads(json), 100 + 50 + 10 + 999);
    }

    #[test]
    fn truncates_to_the_most_recent_seven_distinct_days() {
        let mut entries = Vec::new();
        for day in 1..=10u32 {
            entries.push(format!(
                r#"{{"version":1,"downloads":{},"date":"2024-01-{:02}"}}"#,
                day, day
            ));
        }
        let json = format!(r#"{{"version_downloads":[{}]}}"#, entries.join(","));
        // Days 4..=10 are the most recent 7 distinct dates: downloads 4+5+...+10 = 49.
        assert_eq!(parse_cargo_weekly_downloads(&json), 49);
    }

    #[test]
    fn garbage_downloads_body_is_zero() {
        assert_eq!(parse_cargo_weekly_downloads("nope"), 0);
        assert_eq!(parse_cargo_weekly_downloads(r#"{"nope":true}"#), 0);
    }
}
