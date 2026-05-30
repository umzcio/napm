use super::InstalledTool;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::process::Command;
use serde_json::Value;

/// Merge `pip list --format=json` (installed) with
/// `pip list --outdated --format=json` (latest), keyed by lowercased name.
pub fn parse_pip(list_json: &str, outdated_json: &str) -> Vec<InstalledTool> {
    // key = lowercased name -> (display name, installed, latest)
    let mut map: BTreeMap<String, (String, Option<String>, String)> = BTreeMap::new();

    if let Ok(list) = serde_json::from_str::<Value>(list_json) {
        if let Some(arr) = list.as_array() {
            for p in arr {
                let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let ver = p.get("version").and_then(|v| v.as_str()).unwrap_or("");
                if name.is_empty() || ver.is_empty() {
                    continue;
                }
                map.insert(
                    name.to_lowercase(),
                    (name.to_string(), Some(ver.to_string()), ver.to_string()),
                );
            }
        }
    }

    if let Ok(od) = serde_json::from_str::<Value>(outdated_json) {
        if let Some(arr) = od.as_array() {
            for p in arr {
                let name = match p.get("name").and_then(|v| v.as_str()) {
                    Some(n) if !n.is_empty() => n,
                    _ => continue,
                };
                let cur = p.get("version").and_then(|v| v.as_str());
                let latest = p.get("latest_version").and_then(|v| v.as_str()).unwrap_or("");
                match map.entry(name.to_lowercase()) {
                    Entry::Occupied(mut o) => {
                        let e = o.get_mut();
                        if let Some(c) = cur {
                            e.1 = Some(c.to_string());
                        }
                        if !latest.is_empty() {
                            e.2 = latest.to_string();
                        }
                    }
                    Entry::Vacant(v) => {
                        if !latest.is_empty() {
                            v.insert((name.to_string(), cur.map(|s| s.to_string()), latest.to_string()));
                        }
                    }
                }
            }
        }
    }

    map.into_iter()
        .map(|(_, (name, installed, latest))| InstalledTool {
            name: name.clone(),
            eco: "pip".to_string(),
            pkg: name,
            installed,
            latest,
            size: String::new(),
            pinned: false,
        })
        .collect()
}

/// Find a working pip binary. This machine has `pip3` but no `pip`.
fn pip_bin() -> Option<&'static str> {
    for c in ["pip3", "pip"] {
        let ok = Command::new(c)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return Some(c);
        }
    }
    None
}

/// Run the real pip commands and merge. Returns empty if no pip is installed.
pub fn scan_pip() -> Vec<InstalledTool> {
    let bin = match pip_bin() {
        Some(b) => b,
        None => return Vec::new(),
    };
    let list = super::run(bin, &["list", "--format=json"]);
    let outdated = super::run(bin, &["list", "--outdated", "--format=json"]);
    parse_pip(&list, &outdated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_package_has_equal_installed_and_latest() {
        let list = r#"[{"name":"absl-py","version":"2.3.1"}]"#;
        let rows = parse_pip(list, "[]");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].eco, "pip");
        assert_eq!(rows[0].pkg, "absl-py");
        assert_eq!(rows[0].installed.as_deref(), Some("2.3.1"));
        assert_eq!(rows[0].latest, "2.3.1");
    }

    #[test]
    fn outdated_package_takes_latest_version() {
        let list = r#"[{"name":"altgraph","version":"0.17.2"}]"#;
        let outdated = r#"[{"name":"altgraph","version":"0.17.2","latest_version":"0.17.5"}]"#;
        let rows = parse_pip(list, outdated);
        assert_eq!(rows[0].installed.as_deref(), Some("0.17.2"));
        assert_eq!(rows[0].latest, "0.17.5");
    }

    #[test]
    fn merge_is_case_insensitive() {
        let list = r#"[{"name":"Flask","version":"3.0.0"}]"#;
        let outdated = r#"[{"name":"flask","version":"3.0.0","latest_version":"3.1.0"}]"#;
        let rows = parse_pip(list, outdated);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].latest, "3.1.0");
        assert_eq!(rows[0].name, "Flask");
    }

    #[test]
    fn empty_or_garbage_yields_no_rows() {
        assert!(parse_pip("", "").is_empty());
        assert!(parse_pip("not json", "[]").is_empty());
    }
}
