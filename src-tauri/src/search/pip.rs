use super::SearchResult;
use serde_json::Value;

/// Parse a PyPI project JSON into a single result. None on parse failure or
/// missing info (a 404 body will not parse to a usable info block).
pub fn parse_pypi(json: &str) -> Option<SearchResult> {
    let v: Value = serde_json::from_str(json).ok()?;
    let info = v.get("info")?;
    let name = info.get("name").and_then(|x| x.as_str())?;
    if name.is_empty() { return None; }
    let version = info.get("version").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let description = info.get("summary").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    Some(SearchResult {
        name: name.to_string(), eco: "pip".into(), pkg: name.to_string(),
        version, weekly_downloads: 0, size: String::new(), description,
    })
}

/// Last-week downloads from a pypistats `recent` response, or 0.
pub fn parse_pip_downloads(json: &str) -> u64 {
    serde_json::from_str::<Value>(json).ok()
        .and_then(|v| v.get("data").and_then(|d| d.get("last_week")).and_then(|x| x.as_u64()))
        .unwrap_or(0)
}

/// Exact-name PyPI lookup. Not fuzzy: PyPI removed its search API.
/// Returns a single result if the name resolves, empty if not found.
/// The pip source label in the UI explains the exact-match limitation.
pub fn search_pip(query: &str) -> Vec<SearchResult> {
    let body = match crate::http::get(&format!("https://pypi.org/pypi/{}/json", crate::http::encode(query))) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let mut r = match parse_pypi(&body) {
        Some(r) => r,
        None => return Vec::new(),
    };
    // Fetch weekly downloads from pypistats; failure leaves 0.
    let dl_url = format!(
        "https://pypistats.org/api/packages/{}/recent",
        crate::http::encode(&r.pkg.to_lowercase())
    );
    if let Ok(dl_body) = crate::http::get(&dl_url) {
        r.weekly_downloads = parse_pip_downloads(&dl_body);
    }
    vec![r]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pypi_info() {
        let json = r#"{"info":{"name":"httpie","version":"3.2.2","summary":"Human-friendly HTTP client"}}"#;
        let r = parse_pypi(json).unwrap();
        assert_eq!(r.pkg, "httpie");
        assert_eq!(r.eco, "pip");
        assert_eq!(r.version, "3.2.2");
    }

    #[test]
    fn miss_or_garbage_is_none() {
        assert!(parse_pypi(r#"{"message":"Not Found"}"#).is_none());
        assert!(parse_pypi("nope").is_none());
    }

    #[test]
    fn parses_last_week_downloads() {
        assert_eq!(parse_pip_downloads(r#"{"data":{"last_day":1,"last_week":1200000,"last_month":9}}"#), 1200000);
        assert_eq!(parse_pip_downloads("nope"), 0);
    }
}
