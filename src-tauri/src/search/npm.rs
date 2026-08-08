use super::SearchResult;
use serde_json::Value;
use std::collections::BTreeMap;

/// Parse the npm registry search response into results (downloads filled in
/// later by the wrapper; size is not exposed by npm search so it stays "").
pub fn parse_npm_search(json: &str) -> Vec<SearchResult> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let objects = match v.get("objects").and_then(|o| o.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for o in objects {
        let p = match o.get("package") {
            Some(p) => p,
            None => continue,
        };
        let name = p.get("name").and_then(|x| x.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let version = p
            .get("version")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let description = p
            .get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        out.push(SearchResult {
            name: name.to_string(),
            eco: "npm".into(),
            pkg: name.to_string(),
            version,
            weekly_downloads: 0,
            size: String::new(),
            description,
        });
    }
    out
}

/// Parse a npm downloads-point response. Handles the bulk shape
/// (`{"pkg":{"downloads":N,...}}`) and the single shape
/// (`{"downloads":N,"package":"pkg"}`). Returns pkg -> weekly downloads.
pub fn parse_downloads(json: &str) -> BTreeMap<String, u64> {
    let mut map = BTreeMap::new();
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return map,
    };
    // Single shape: has a top-level "package" string + "downloads" number.
    if let (Some(pkg), Some(dl)) = (
        v.get("package").and_then(|x| x.as_str()),
        v.get("downloads").and_then(|x| x.as_u64()),
    ) {
        map.insert(pkg.to_string(), dl);
        return map;
    }
    // Bulk shape: object of pkg -> {downloads, package}. Null entries = unknown pkg.
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            if let Some(dl) = val.get("downloads").and_then(|x| x.as_u64()) {
                map.insert(k.clone(), dl);
            }
        }
    }
    map
}

/// Federated npm search: calls the npm registry search endpoint, then bulk-fetches
/// weekly download counts and fills them in. Scoped packages are fetched individually.
/// Any network failure degrades gracefully to an empty list or zero downloads.
pub fn search_npm(query: &str) -> Vec<SearchResult> {
    let url = format!(
        "https://registry.npmjs.org/-/v1/search?text={}&size=25",
        crate::http::encode(query)
    );
    let body = match crate::http::get(&url) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let mut rows = parse_npm_search(&body);
    if rows.is_empty() {
        return rows;
    }

    // Split into unscoped (no leading '@') and scoped packages.
    let unscoped: Vec<String> = rows
        .iter()
        .filter(|r| !r.pkg.starts_with('@'))
        .map(|r| r.pkg.clone())
        .collect();
    let scoped: Vec<String> = rows
        .iter()
        .filter(|r| r.pkg.starts_with('@'))
        .map(|r| r.pkg.clone())
        .collect();

    let mut dl_map: BTreeMap<String, u64> = BTreeMap::new();

    // Bulk-fetch unscoped packages in one call.
    if !unscoped.is_empty() {
        let joined = unscoped.join(",");
        let dl_url = format!("https://api.npmjs.org/downloads/point/last-week/{}", joined);
        if let Ok(dl_body) = crate::http::get(&dl_url) {
            for (k, v) in parse_downloads(&dl_body) {
                dl_map.insert(k, v);
            }
        }
    }

    // Fetch each scoped package concurrently (the bulk endpoint cannot combine
    // scoped names). The shared agent keeps connections warm across them.
    if !scoped.is_empty() {
        let maps: Vec<BTreeMap<String, u64>> = std::thread::scope(|s| {
            let handles: Vec<_> = scoped
                .iter()
                .map(|pkg| {
                    s.spawn(move || {
                        let dl_url = format!(
                            "https://api.npmjs.org/downloads/point/last-week/{}",
                            crate::http::encode(pkg)
                        );
                        match crate::http::get(&dl_url) {
                            Ok(dl_body) => parse_downloads(&dl_body),
                            Err(_) => BTreeMap::new(),
                        }
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_default())
                .collect()
        });
        for m in maps {
            for (k, v) in m {
                dl_map.insert(k, v);
            }
        }
    }

    // Populate weekly_downloads from the map.
    for row in &mut rows {
        if let Some(&dl) = dl_map.get(&row.pkg) {
            row.weekly_downloads = dl;
        }
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_version_description() {
        let json = r#"{"objects":[
            {"package":{"name":"eslint","version":"9.10.0","description":"Pluggable JS linter"}},
            {"package":{"name":"@scope/x","version":"1.2.3","description":""}}
        ]}"#;
        let r = parse_npm_search(json);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].pkg, "eslint");
        assert_eq!(r[0].eco, "npm");
        assert_eq!(r[0].version, "9.10.0");
        assert_eq!(r[1].pkg, "@scope/x");
    }

    #[test]
    fn garbage_yields_no_rows() {
        assert!(parse_npm_search("nope").is_empty());
        assert!(parse_npm_search(r#"{"objects":[]}"#).is_empty());
    }

    #[test]
    fn parses_bulk_and_single_downloads() {
        let bulk = r#"{"eslint":{"downloads":32000000,"package":"eslint"},
                       "prettier":{"downloads":28000000,"package":"prettier"},
                       "bogus":null}"#;
        let m = parse_downloads(bulk);
        assert_eq!(m.get("eslint"), Some(&32000000));
        assert_eq!(m.get("prettier"), Some(&28000000));
        assert_eq!(m.get("bogus"), None);

        let single = r#"{"downloads":1400000,"package":"@anthropic-ai/claude-code"}"#;
        let m2 = parse_downloads(single);
        assert_eq!(m2.get("@anthropic-ai/claude-code"), Some(&1400000));
    }
}
