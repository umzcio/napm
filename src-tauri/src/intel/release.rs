use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Convert a leading ISO 8601 datetime ("2024-05-01T12:00:00Z" or
/// "2024-05-01...") to Unix seconds (UTC). Returns None if the date part does
/// not parse. Seconds precision; ignores any fractional/zone suffix.
pub fn iso_to_unix(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 10 {
        return None;
    }
    let num = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let (mut hh, mut mm, mut ss) = (0i64, 0i64, 0i64);
    if s.len() >= 19 && b[10] == b'T' {
        hh = num(11, 13).unwrap_or(0);
        mm = num(14, 16).unwrap_or(0);
        ss = num(17, 19).unwrap_or(0);
    }
    // days_from_civil (Howard Hinnant): days since 1970-01-01.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

/// npm registry document -> publish unix time for `version` (from `time[version]`).
pub fn parse_npm_time(json: &str, version: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(json).ok()?;
    let t = v.get("time")?.get(version)?.as_str()?;
    iso_to_unix(t)
}

/// PyPI project document -> upload unix time for `version`
/// (`releases[version][0].upload_time_iso_8601`).
pub fn parse_pypi_time(json: &str, version: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(json).ok()?;
    let files = v.get("releases")?.get(version)?.as_array()?;
    let t = files
        .first()?
        .get("upload_time_iso_8601")
        .or_else(|| files.first()?.get("upload_time"))?
        .as_str()?;
    iso_to_unix(t)
}

/// (recommendation, age_label) from a publish time and "now". Settled (>= 7 days)
/// is "safe"; fresher is "new". A None publish time is "unknown".
pub fn age_verdict(published: Option<i64>, now: i64) -> (String, String) {
    let p = match published {
        Some(p) => p,
        None => return ("unknown".into(), "".into()),
    };
    let days = ((now - p).max(0)) / 86400;
    let label = if days < 1 {
        "released today".to_string()
    } else if days == 1 {
        "released 1 day ago".to_string()
    } else {
        format!("released {} days ago", days)
    };
    let rec = if days > 7 { "safe" } else { "new" };
    (rec.to_string(), label)
}

/// GitHub Search API response -> `total_count`.
pub fn parse_search_total_count(json: &str) -> Option<u64> {
    serde_json::from_str::<Value>(json)
        .ok()?
        .get("total_count")?
        .as_u64()
}

/// Unix seconds (UTC) -> "YYYY-MM-DD" (inverse of the civil-day math in
/// iso_to_unix; Howard Hinnant's civil_from_days).
pub fn unix_to_ymd(unix: i64) -> String {
    let days = unix.div_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", year, m, d)
}

/// Decide whether a fresh release should be held, from GitHub issue counts.
/// Pure and unit-testable. `recent` = issues opened since the release,
/// `baseline` = issues opened in the 90 days before it, `days_since` = days the
/// release has been out (floored at 1). Returns Some(multiple) when it should
/// hold (>= 2x the daily baseline AND at least 3 new issues, with a real prior
/// baseline), else None.
pub fn velocity_hold(recent: u64, baseline: u64, days_since: i64) -> Option<u64> {
    if recent < 3 {
        return None;
    }
    let baseline_daily = baseline as f64 / 90.0;
    if baseline_daily <= 0.0 {
        return None;
    }
    let recent_daily = recent as f64 / days_since.max(1) as f64;
    let mult = recent_daily / baseline_daily;
    if mult >= 2.0 {
        Some(mult.round() as u64)
    } else {
        None
    }
}

/// Extract (owner, repo) from a GitHub URL in any common form
/// (git+https://github.com/owner/repo.git, https://github.com/owner/repo, etc).
pub fn github_repo_from_url(url: &str) -> Option<(String, String)> {
    let i = url.find("github.com/")? + "github.com/".len();
    let rest = &url[i..];
    let mut parts = rest.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim().trim_end_matches(".git");
    // strip any trailing query/fragment/path on the repo segment
    let repo = repo.split(['#', '?']).next().unwrap_or(repo);
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// Pull changelog bullet lines from a GitHub releases array for the release whose
/// tag matches `version` (with or without a leading "v"). Returns up to 12
/// non-empty, de-marked lines from that release body.
pub fn parse_github_releases(json: &str, version: &str) -> Vec<String> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match v.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let want = version.trim_start_matches('v');
    let body = arr.iter().find_map(|rel| {
        let tag = rel.get("tag_name").and_then(|t| t.as_str()).unwrap_or("");
        if tag.trim_start_matches('v') == want {
            rel.get("body").and_then(|b| b.as_str())
        } else {
            None
        }
    });
    let body = match body {
        Some(b) => b,
        None => return Vec::new(),
    };
    body.lines()
        .map(|l| l.trim().trim_start_matches(['#', '-', '*', ' ']).trim())
        .filter(|l| !l.is_empty())
        .take(12)
        .map(String::from)
        .collect()
}

/// Extract a GitHub repo URL from an already-fetched registry doc for `eco`.
fn repo_url_from_doc(eco: &str, body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    match eco {
        "npm" | "npx" => v.get("repository")?.get("url")?.as_str().map(String::from),
        "pip" => {
            let info = v.get("info")?;
            if let Some(urls) = info.get("project_urls").and_then(|u| u.as_object()) {
                for (_, val) in urls {
                    if let Some(s) = val.as_str() {
                        if s.contains("github.com") {
                            return Some(s.to_string());
                        }
                    }
                }
            }
            info.get("home_page")
                .and_then(|h| h.as_str())
                .filter(|s| s.contains("github.com"))
                .map(String::from)
        }
        _ => None,
    }
}

/// (recommendation, age_label, reason) for a package version. npm/npx and pip
/// fetch the registry doc once, derive the publish time, and - only when the
/// release is fresh ("new") - run a token-gated GitHub issue-velocity check that
/// can upgrade the verdict to "hold". brew has no per-version date: ("unknown","","").
pub fn release_verdict(
    eco: &str,
    pkg: &str,
    version: &str,
    now: i64,
    cache_dir: &Path,
) -> (String, String, String) {
    let doc = match eco {
        "npm" | "npx" | "pip" => super::registry::doc(eco, pkg, cache_dir),
        _ => None,
    };
    let published = doc.as_deref().and_then(|b| match eco {
        "npm" | "npx" => parse_npm_time(b, version),
        "pip" => parse_pypi_time(b, version),
        _ => None,
    });
    let (rec, age_label) = age_verdict(published, now);
    if rec != "new" {
        return (rec, age_label, String::new());
    }
    // Fresh: try to upgrade to "hold" from issue velocity. Any miss stays "new".
    let upgraded = (|| {
        let body = doc.as_deref()?;
        let repo_url = repo_url_from_doc(eco, body)?;
        let (owner, repo) = github_repo_from_url(&repo_url)?;
        velocity_verdict(eco, pkg, version, &owner, &repo, published?, now, cache_dir)
    })();
    match upgraded {
        Some((r, reason)) => (r, age_label, reason),
        None => (rec, age_label, String::new()),
    }
}

#[derive(Serialize, Deserialize)]
struct HoldCache {
    checked_ts: i64,
    recommendation: String,
    reason: String,
}

/// GitHub issue-velocity check for a fresh release. Returns Some(("hold", reason))
/// when issues are opening fast enough to warrant holding, else None (stay "new").
/// Token-gated (no token -> None), cached 12h per (eco,pkg,version). Any HTTP
/// failure returns None and is not cached. `published` is the release unix time.
// Each parameter is an independently-sourced piece of release identity/context;
// bundling them into a struct would not clarify the call sites in this module.
#[allow(clippy::too_many_arguments)]
fn velocity_verdict(
    eco: &str,
    pkg: &str,
    version: &str,
    owner: &str,
    repo: &str,
    published: i64,
    now: i64,
    cache_dir: &Path,
) -> Option<(String, String)> {
    // Cache (12h TTL) so velocity is re-checked through the fresh window.
    let safe_eco = eco.replace(['/', '\\'], "_").replace("..", "_");
    let safe_pkg = pkg.replace(['/', '@', '\\'], "_").replace("..", "_");
    let safe_ver = version.replace(['/', '\\'], "_").replace("..", "_");
    let cache_file = cache_dir.join(format!("hold_{}_{}_{}.json", safe_eco, safe_pkg, safe_ver));
    if let Ok(s) = std::fs::read_to_string(&cache_file) {
        if let Ok(c) = serde_json::from_str::<HoldCache>(&s) {
            if now - c.checked_ts < 12 * 3600 {
                return if c.recommendation == "hold" {
                    Some((c.recommendation, c.reason))
                } else {
                    None
                };
            }
        }
    }

    let token = super::github_token(cache_dir)?; // no token -> stay "new"
    let auth = format!("Bearer {}", token);
    let headers: Vec<(&str, &str)> = vec![
        ("Accept", "application/vnd.github+json"),
        ("Authorization", &auth),
    ];

    let release_ymd = unix_to_ymd(published);
    let start_ymd = unix_to_ymd(published - 90 * 86400);
    let count = |q: String| -> Option<u64> {
        let url = format!(
            "https://api.github.com/search/issues?q={}&per_page=1",
            crate::http::encode(&q)
        );
        let body = crate::http::get_with_headers(&url, &headers).ok()?;
        parse_search_total_count(&body)
    };
    let recent = count(format!(
        "repo:{}/{} type:issue created:>={}",
        owner, repo, release_ymd
    ))?;
    let baseline = count(format!(
        "repo:{}/{} type:issue created:{}..{}",
        owner, repo, start_ymd, release_ymd
    ))?;
    let days_since = ((now - published).max(0)) / 86400;

    let (rec, reason) = match velocity_hold(recent, baseline, days_since) {
        Some(mult) => (
            "hold".to_string(),
            format!(
                "issues opening about {}x faster than usual since release",
                mult
            ),
        ),
        None => ("new".to_string(), String::new()),
    };
    if let Ok(s) = serde_json::to_string(&HoldCache {
        checked_ts: now,
        recommendation: rec.clone(),
        reason: reason.clone(),
    }) {
        let _ = std::fs::write(&cache_file, s);
    }
    if rec == "hold" {
        Some((rec, reason))
    } else {
        None
    }
}

/// Fetch and cache the GitHub changelog for (eco, pkg, version).
/// Cache is permanent (one write per unique version). Any failure returns
/// Vec::new(). Caches empty results too, to avoid re-hitting a rate limit.
pub fn changelog(eco: &str, pkg: &str, version: &str, cache_dir: &Path) -> Vec<String> {
    // Build a filesystem-safe cache key. Sanitize eco, pkg, and version to
    // prevent path traversal via frontend-supplied strings.
    let safe_eco = eco.replace(['/', '\\'], "_").replace("..", "_");
    let safe_pkg = pkg.replace(['/', '@', '\\'], "_").replace("..", "_");
    let safe_ver = version.replace(['/', '\\'], "_").replace("..", "_");
    let cache_file = cache_dir.join(format!(
        "changelog_{}_{}_{}.json",
        safe_eco, safe_pkg, safe_ver
    ));

    // Return cached result if it exists (permanent cache per version).
    if let Ok(s) = std::fs::read_to_string(&cache_file) {
        if let Ok(v) = serde_json::from_str::<Vec<String>>(&s) {
            return v;
        }
    }

    // Derive the GitHub repo URL from the registry (npm/pip go through the
    // shared registry-document cache since release_verdict already fetched
    // the same document once per TTL window).
    let repo_url: Option<String> = match eco {
        "npm" | "npx" | "pip" => {
            super::registry::doc(eco, pkg, cache_dir).and_then(|body| repo_url_from_doc(eco, &body))
        }
        "brew" => {
            let url = format!(
                "https://formulae.brew.sh/api/formula/{}.json",
                crate::http::encode(pkg)
            );
            crate::http::get(&url).ok().and_then(|body| {
                let v: Value = serde_json::from_str(&body).ok()?;
                v.get("homepage")?
                    .as_str()
                    .filter(|s| s.contains("github.com"))
                    .map(String::from)
            })
        }
        _ => None,
    };

    // fetch_result: Ok(vec) means the HTTP call succeeded (notes may be empty);
    // Err(()) means the HTTP call itself failed (network / non-2xx).
    let fetch_result: Result<Vec<String>, ()> = (|| {
        let (owner, repo) = github_repo_from_url(repo_url.as_deref()?)?;
        let releases_url = format!(
            "https://api.github.com/repos/{}/{}/releases?per_page=20",
            owner, repo
        );
        let mut headers: Vec<(&str, &str)> = vec![("Accept", "application/vnd.github+json")];
        // Read the token at call time; store it in a local binding so the reference lives long enough.
        let token_str: String;
        let token_header: String;
        if let Some(token) = super::github_token(cache_dir) {
            token_str = token;
            token_header = format!("Bearer {}", token_str);
            headers.push(("Authorization", &token_header));
        }
        // Propagate HTTP errors as None so the outer closure returns None -> Err(()).
        let body = crate::http::get_with_headers(&releases_url, &headers).ok()?;
        Some(parse_github_releases(&body, version))
    })()
    .map(Ok)
    .unwrap_or(Err(()));

    match fetch_result {
        Ok(result) => {
            // Successful HTTP response: cache even if no matching release notes were
            // found (a legitimate empty result), to avoid hammering the API.
            if let Ok(s) = serde_json::to_string(&result) {
                let _ = std::fs::write(&cache_file, s);
            }
            result
        }
        Err(()) => {
            // HTTP call failed (network error, rate-limit, etc.): do NOT cache so
            // the next run retries.
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_parses_known_epochs() {
        assert_eq!(iso_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_unix("2000-01-01T00:00:00Z"), Some(946684800));
        assert_eq!(iso_to_unix("2024-05-01T12:00:00Z"), Some(1714564800));
        assert_eq!(iso_to_unix("nope"), None);
    }

    #[test]
    fn npm_and_pypi_times_and_verdict() {
        let npm = r#"{"time":{"1.2.3":"2024-05-01T12:00:00Z","modified":"x"}}"#;
        assert_eq!(parse_npm_time(npm, "1.2.3"), Some(1714564800));
        let pypi = r#"{"releases":{"1.0.0":[{"upload_time_iso_8601":"2024-05-01T12:00:00Z"}]}}"#;
        assert_eq!(parse_pypi_time(pypi, "1.0.0"), Some(1714564800));

        let now = 1714564800 + 10 * 86400; // 10 days later
        let (rec, label) = age_verdict(Some(1714564800), now);
        assert_eq!(rec, "safe");
        assert_eq!(label, "released 10 days ago");
        let (rec2, _) = age_verdict(Some(now - 3 * 86400), now);
        assert_eq!(rec2, "new");
        assert_eq!(age_verdict(None, now).0, "unknown");
        // Boundary: exactly 7 days is "new" (spec: "more than 7 days" = safe).
        let (rec7, _) = age_verdict(Some(now - 7 * 86400), now);
        assert_eq!(rec7, "new");
        // Exactly 8 days is "safe".
        let (rec8, _) = age_verdict(Some(now - 8 * 86400), now);
        assert_eq!(rec8, "safe");
    }

    #[test]
    fn parses_search_total_and_formats_date() {
        assert_eq!(
            parse_search_total_count(r#"{"total_count":42,"items":[]}"#),
            Some(42)
        );
        assert_eq!(parse_search_total_count("not json"), None);
        assert_eq!(unix_to_ymd(1714564800), "2024-05-01");
        assert_eq!(unix_to_ymd(0), "1970-01-01");
    }

    #[test]
    fn velocity_hold_decisions() {
        // Clear spike: 20 issues in 2 days (10/day) vs 0.1/day baseline -> hold.
        assert_eq!(velocity_hold(20, 9, 2), Some(100));
        // Exactly 2x with >=3 issues -> hold, multiple 2.
        assert_eq!(velocity_hold(6, 270, 1), Some(2));
        // Below 2x -> none. 10/day vs 10/day baseline.
        assert_eq!(velocity_hold(10, 900, 1), None);
        // Absolute floor: fewer than 3 new issues -> none.
        assert_eq!(velocity_hold(2, 0, 1), None);
        // No prior issue history (baseline 0) -> none (insufficient signal).
        assert_eq!(velocity_hold(5, 0, 1), None);
        // days_since is floored at 1 (never divide by zero).
        assert_eq!(velocity_hold(6, 270, 0), Some(2));
    }

    #[test]
    fn repo_url_and_release_notes() {
        assert_eq!(
            github_repo_from_url("git+https://github.com/eslint/eslint.git"),
            Some(("eslint".to_string(), "eslint".to_string()))
        );
        assert_eq!(
            github_repo_from_url("https://github.com/cli/cli/tree/trunk"),
            Some(("cli".to_string(), "cli".to_string()))
        );
        assert_eq!(github_repo_from_url("https://example.com/x"), None);

        let rel = "[
          {\"tag_name\":\"v1.2.3\",\"body\":\"# Notes\\n- Fixed a bug\\n- Added a flag\\n\"},
          {\"tag_name\":\"v1.2.2\",\"body\":\"old\"}
        ]";
        let log = parse_github_releases(rel, "1.2.3");
        assert_eq!(
            log,
            vec![
                "Notes".to_string(),
                "Fixed a bug".to_string(),
                "Added a flag".to_string()
            ]
        );
    }
}
