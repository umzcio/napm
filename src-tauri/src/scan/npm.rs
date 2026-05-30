use super::InstalledTool;
use std::collections::BTreeMap;
use std::process::Command;
use serde_json::Value;

/// Merge `npm ls -g --json` (installed) with `npm outdated -g --json` (latest),
/// mirroring reference/scanner.js scanNpm(). BTreeMap keeps output stable/sorted.
pub fn parse_npm(ls_json: &str, outdated_json: &str) -> Vec<InstalledTool> {
    // (installed, latest) per package key
    let mut map: BTreeMap<String, (Option<String>, String)> = BTreeMap::new();

    if let Ok(ls) = serde_json::from_str::<Value>(ls_json) {
        if let Some(deps) = ls.get("dependencies").and_then(|d| d.as_object()) {
            for (pkg, info) in deps {
                if let Some(ver) = info.get("version").and_then(|v| v.as_str()) {
                    map.insert(pkg.clone(), (Some(ver.to_string()), ver.to_string()));
                }
            }
        }
    }

    if let Ok(od) = serde_json::from_str::<Value>(outdated_json) {
        if let Some(obj) = od.as_object() {
            for (pkg, info) in obj {
                let latest = info.get("latest").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let current = info.get("current").and_then(|v| v.as_str()).map(|s| s.to_string());
                let entry = map.entry(pkg.clone()).or_insert((current.clone(), latest.clone()));
                if current.is_some() {
                    entry.0 = current;
                }
                if !latest.is_empty() {
                    entry.1 = latest;
                }
            }
        }
    }

    map.into_iter()
        .map(|(pkg, (installed, latest))| InstalledTool {
            name: pkg.clone(),
            eco: "npm".to_string(),
            pkg,
            installed,
            latest,
            size: String::new(),
            pinned: false,
        })
        .collect()
}

/// Run the real npm commands and merge. `npm outdated` exits non-zero when
/// results exist, so we read stdout regardless of exit status.
pub fn scan_npm() -> Vec<InstalledTool> {
    let ls = run_npm(&["ls", "-g", "--depth=0", "--json"]);
    let outdated = run_npm(&["outdated", "-g", "--json"]);
    parse_npm(&ls, &outdated)
}

fn run_npm(args: &[&str]) -> String {
    Command::new("npm")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_tool_has_equal_installed_and_latest() {
        let ls = r#"{"dependencies":{"typescript":{"version":"5.5.4"}}}"#;
        let outdated = r#"{}"#;
        let rows = parse_npm(ls, outdated);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pkg, "typescript");
        assert_eq!(rows[0].eco, "npm");
        assert_eq!(rows[0].installed.as_deref(), Some("5.5.4"));
        assert_eq!(rows[0].latest, "5.5.4");
        assert_eq!(rows[0].pinned, false);
    }

    #[test]
    fn outdated_tool_takes_latest_from_outdated() {
        let ls = r#"{"dependencies":{"typescript":{"version":"5.5.4"}}}"#;
        let outdated = r#"{"typescript":{"current":"5.5.4","latest":"5.6.2"}}"#;
        let rows = parse_npm(ls, outdated);
        assert_eq!(rows[0].installed.as_deref(), Some("5.5.4"));
        assert_eq!(rows[0].latest, "5.6.2");
    }

    #[test]
    fn scoped_package_name_is_preserved() {
        let ls = r#"{"dependencies":{"@anthropic-ai/claude-code":{"version":"2.1.158"}}}"#;
        let rows = parse_npm(ls, "{}");
        assert_eq!(rows[0].pkg, "@anthropic-ai/claude-code");
        assert_eq!(rows[0].name, "@anthropic-ai/claude-code");
    }

    #[test]
    fn empty_or_garbage_output_yields_no_rows() {
        assert!(parse_npm("", "").is_empty());
        assert!(parse_npm("not json", "also not json").is_empty());
    }
}
