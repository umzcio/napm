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
            description: String::new(),
            updated: 0,
        })
        .collect()
}

/// Build a name -> (publisher-handle, description) map from
/// `brew info --json=v2 --installed`. Publisher is the GitHub/GitLab owner of
/// the homepage (else its domain label); description is the formula `desc`.
pub fn parse_brew_info(info_json: &str) -> BTreeMap<String, (String, String)> {
    let mut map = BTreeMap::new();
    if let Ok(v) = serde_json::from_str::<Value>(info_json) {
        if let Some(formulae) = v.get("formulae").and_then(|f| f.as_array()) {
            for f in formulae {
                let name = match f.get("name").and_then(|n| n.as_str()) {
                    Some(n) if !n.is_empty() => n,
                    _ => continue,
                };
                let homepage = f.get("homepage").and_then(|h| h.as_str()).unwrap_or("");
                let publisher = super::publisher::publisher_from_homepage(homepage)
                    .and_then(|o| super::publisher::to_handle(&o))
                    .unwrap_or_default();
                let desc = f
                    .get("desc")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                map.insert(name.to_string(), (publisher, desc));
            }
        }
    }
    map
}

/// Recorded install time of a keg from its INSTALL_RECEIPT.json `time`, falling
/// back to the keg directory's mtime.
fn brew_install_time(keg: &std::path::Path) -> i64 {
    if let Ok(s) = std::fs::read_to_string(keg.join("INSTALL_RECEIPT.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if let Some(t) = v.get("time").and_then(|x| x.as_i64()) {
                return t;
            }
        }
    }
    super::path_mtime(keg)
}

/// Run the real brew commands and merge, then enrich publisher, description,
/// size (the installed keg), and install time.
pub fn scan_brew() -> Vec<InstalledTool> {
    let list = super::run("brew", &["list", "--versions"]);
    let outdated = super::run("brew", &["outdated", "--json=v2"]);
    let mut rows = parse_brew(&list, &outdated);

    let meta = parse_brew_info(&super::run("brew", &["info", "--json=v2", "--installed"]));
    let prefix = super::run("brew", &["--prefix"]);
    let prefix = prefix.trim();
    for row in rows.iter_mut() {
        if let Some((publisher, desc)) = meta.get(&row.pkg) {
            row.publisher = publisher.clone();
            row.description = desc.clone();
        }
        if !prefix.is_empty() {
            if let Some(ver) = &row.installed {
                let keg = std::path::Path::new(prefix)
                    .join("Cellar")
                    .join(&row.pkg)
                    .join(ver);
                row.size = super::size::human_size(super::size::dir_size(&keg));
                row.updated = brew_install_time(&keg);
            }
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
    fn info_yields_publisher_and_description() {
        let info = r#"{"formulae":[
            {"name":"ripgrep","desc":"Search tool like grep","homepage":"https://github.com/BurntSushi/ripgrep"},
            {"name":"openssl","desc":"Cryptography toolkit","homepage":"https://www.openssl.org/"}
        ]}"#;
        let meta = parse_brew_info(info);
        let rg = meta.get("ripgrep").unwrap();
        assert_eq!(rg.0, "burntsushi");
        assert_eq!(rg.1, "Search tool like grep");
        assert_eq!(meta.get("openssl").unwrap().0, "openssl"); // domain fallback
    }
}
