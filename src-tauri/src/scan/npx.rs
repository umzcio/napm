use super::InstalledTool;
use std::collections::BTreeMap;
use std::fs;
use serde_json::Value;

/// Strip the trailing `@version` from an npx package spec, preserving scoped
/// names. "pkg@1.2.3" -> "pkg"; "@scope/pkg@1.2.3" -> "@scope/pkg".
pub fn npx_pkg_name(spec: &str) -> &str {
    match spec.rfind('@') {
        Some(i) if i > 0 => &spec[..i],
        _ => spec,
    }
}

/// Collapse (name, version, publisher) triples into library rows, deduping by
/// name and keeping the greatest version string (and that version's publisher).
/// latest is set equal to installed: in M2 napm does not know the registry
/// latest for npx tools, so this is a neutral sentinel meaning "freshness
/// unknown" (rendered as such in the UI).
pub fn dedup_npx(items: Vec<(String, String, String)>) -> Vec<InstalledTool> {
    // name -> (version, publisher)
    let mut map: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (name, ver, publisher) in items {
        map.entry(name)
            .and_modify(|e| {
                if ver > e.0 {
                    e.0 = ver.clone();
                    e.1 = publisher.clone();
                }
            })
            .or_insert((ver, publisher));
    }
    map.into_iter()
        .map(|(name, (ver, publisher))| InstalledTool {
            name: name.clone(),
            eco: "npx".to_string(),
            pkg: name,
            installed: Some(ver.clone()),
            latest: ver,
            size: String::new(),
            pinned: false,
            publisher,
        })
        .collect()
}

/// Walk ~/.npm/_npx/<hash>/, read each shim's `_npx.packages` to learn which
/// tool was run, and resolve its cached version from node_modules. Returns
/// empty if the cache does not exist.
pub fn scan_npx() -> Vec<InstalledTool> {
    let home = match std::env::var_os("HOME") {
        Some(h) => h,
        None => return Vec::new(),
    };
    let root = std::path::Path::new(&home).join(".npm").join("_npx");
    let entries = match fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut items: Vec<(String, String, String)> = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        let shim = match fs::read_to_string(dir.join("package.json")) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let shim: Value = match serde_json::from_str(&shim) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let specs = match shim.get("_npx").and_then(|n| n.get("packages")).and_then(|p| p.as_array()) {
            Some(s) => s,
            None => continue,
        };
        for spec in specs {
            let spec = match spec.as_str() {
                Some(s) => s,
                None => continue,
            };
            let name = npx_pkg_name(spec);
            let pkg_json = dir.join("node_modules").join(name).join("package.json");
            if let Ok(s) = fs::read_to_string(&pkg_json) {
                if let Ok(v) = serde_json::from_str::<Value>(&s) {
                    if let Some(ver) = v.get("version").and_then(|x| x.as_str()) {
                        let publisher = v
                            .get("author")
                            .and_then(super::publisher::author_from_pkg_json)
                            .and_then(|n| super::publisher::to_handle(&n))
                            .unwrap_or_default();
                        items.push((name.to_string(), ver.to_string(), publisher));
                    }
                }
            }
        }
    }
    dedup_npx(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_version_from_plain_spec() {
        assert_eq!(npx_pkg_name("get-shit-done-cc@latest"), "get-shit-done-cc");
        assert_eq!(npx_pkg_name("typescript@5.6.2"), "typescript");
    }

    #[test]
    fn preserves_scoped_names() {
        assert_eq!(npx_pkg_name("@anthropic-ai/claude-code@1.0.0"), "@anthropic-ai/claude-code");
        assert_eq!(npx_pkg_name("@scope/pkg"), "@scope/pkg");
    }

    #[test]
    fn plain_name_without_version_is_unchanged() {
        assert_eq!(npx_pkg_name("eslint"), "eslint");
    }

    #[test]
    fn dedup_keeps_greatest_version_and_tags_npx() {
        let rows = dedup_npx(vec![
            ("tool".to_string(), "1.0.0".to_string(), "alice".to_string()),
            ("tool".to_string(), "1.2.0".to_string(), "bob".to_string()),
            ("other".to_string(), "0.1.0".to_string(), "carol".to_string()),
        ]);
        assert_eq!(rows.len(), 2);
        let tool = rows.iter().find(|t| t.pkg == "tool").unwrap();
        assert_eq!(tool.eco, "npx");
        assert_eq!(tool.installed.as_deref(), Some("1.2.0"));
        assert_eq!(tool.latest, "1.2.0");
        assert_eq!(tool.publisher, "bob"); // publisher of the chosen (greatest) version
    }
}
