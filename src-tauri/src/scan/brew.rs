use super::InstalledTool;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use serde_json::Value;

/// Merge `brew list --versions` (installed) with `brew outdated --json=v2`
/// (latest). Mirrors reference/scanner.js scanBrew().
pub fn parse_brew(list_versions: &str, outdated_json: &str) -> Vec<InstalledTool> {
    let mut map: BTreeMap<String, (Option<String>, String)> = BTreeMap::new();

    for line in list_versions.lines() {
        let mut parts = line.split_whitespace();
        if let Some(name) = parts.next() {
            // remaining tokens are versions; the last is the newest installed
            if let Some(ver) = parts.last() {
                map.insert(name.to_string(), (Some(ver.to_string()), ver.to_string()));
            }
        }
    }

    if let Ok(od) = serde_json::from_str::<Value>(outdated_json) {
        if let Some(formulae) = od.get("formulae").and_then(|f| f.as_array()) {
            for f in formulae {
                let name = match f.get("name").and_then(|v| v.as_str()) {
                    Some(n) if !n.is_empty() => n,
                    _ => continue,
                };
                let latest = f.get("current_version").and_then(|v| v.as_str()).unwrap_or("");
                let installed = f
                    .get("installed_versions")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.last())
                    .and_then(|v| v.as_str());
                match map.entry(name.to_string()) {
                    Entry::Occupied(mut o) => {
                        let e = o.get_mut();
                        if let Some(i) = installed {
                            e.0 = Some(i.to_string());
                        }
                        if !latest.is_empty() {
                            e.1 = latest.to_string();
                        }
                    }
                    Entry::Vacant(v) => {
                        if !latest.is_empty() {
                            v.insert((installed.map(|s| s.to_string()), latest.to_string()));
                        }
                    }
                }
            }
        }
    }

    map.into_iter()
        .map(|(name, (installed, latest))| InstalledTool {
            name: name.clone(),
            eco: "brew".to_string(),
            pkg: name,
            installed,
            latest,
            size: String::new(),
            pinned: false,
            publisher: String::new(),
        })
        .collect()
}

/// Build a name -> publisher-handle map from `brew info --json=v2 --installed`,
/// taking the GitHub/GitLab owner of each formula's homepage.
pub fn parse_brew_publishers(info_json: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(v) = serde_json::from_str::<Value>(info_json) {
        if let Some(formulae) = v.get("formulae").and_then(|f| f.as_array()) {
            for f in formulae {
                let name = match f.get("name").and_then(|n| n.as_str()) {
                    Some(n) if !n.is_empty() => n,
                    _ => continue,
                };
                let homepage = f.get("homepage").and_then(|h| h.as_str()).unwrap_or("");
                if let Some(handle) = super::publisher::publisher_from_homepage(homepage)
                    .and_then(|o| super::publisher::to_handle(&o))
                {
                    map.insert(name.to_string(), handle);
                }
            }
        }
    }
    map
}

/// Run the real brew commands and merge, then enrich publishers from
/// `brew info --json=v2 --installed`.
pub fn scan_brew() -> Vec<InstalledTool> {
    let list = super::run("brew", &["list", "--versions"]);
    let outdated = super::run("brew", &["outdated", "--json=v2"]);
    let mut rows = parse_brew(&list, &outdated);
    let pubs = parse_brew_publishers(&super::run("brew", &["info", "--json=v2", "--installed"]));
    for row in rows.iter_mut() {
        if let Some(p) = pubs.get(&row.pkg) {
            row.publisher = p.clone();
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_formula_has_equal_installed_and_latest() {
        let list = "aom 3.13.3\nripgrep 14.1.1\n";
        let rows = parse_brew(list, r#"{"formulae":[]}"#);
        let rg = rows.iter().find(|t| t.pkg == "ripgrep").unwrap();
        assert_eq!(rg.eco, "brew");
        assert_eq!(rg.installed.as_deref(), Some("14.1.1"));
        assert_eq!(rg.latest, "14.1.1");
    }

    #[test]
    fn outdated_formula_takes_latest_from_current_version() {
        let list = "aom 3.13.3\n";
        let outdated = r#"{"formulae":[{"name":"aom","installed_versions":["3.13.3"],"current_version":"3.14.1"}]}"#;
        let rows = parse_brew(list, outdated);
        let aom = rows.iter().find(|t| t.pkg == "aom").unwrap();
        assert_eq!(aom.installed.as_deref(), Some("3.13.3"));
        assert_eq!(aom.latest, "3.14.1");
    }

    #[test]
    fn multiple_installed_versions_use_the_last_token() {
        let rows = parse_brew("foo 1.0.0 1.2.0\n", r#"{"formulae":[]}"#);
        assert_eq!(rows[0].installed.as_deref(), Some("1.2.0"));
        assert_eq!(rows[0].latest, "1.2.0");
    }

    #[test]
    fn empty_or_garbage_yields_no_rows() {
        assert!(parse_brew("", "").is_empty());
        assert!(parse_brew("", "not json").is_empty());
    }

    #[test]
    fn publisher_comes_from_formula_homepage() {
        let info = r#"{"formulae":[
            {"name":"ripgrep","homepage":"https://github.com/BurntSushi/ripgrep"},
            {"name":"openssl","homepage":"https://www.openssl.org/"}
        ]}"#;
        let pubs = parse_brew_publishers(info);
        assert_eq!(pubs.get("ripgrep").map(|s| s.as_str()), Some("burntsushi"));
        assert_eq!(pubs.get("openssl"), None); // non-git homepage yields no publisher
    }
}
