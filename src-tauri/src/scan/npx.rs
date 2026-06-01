use super::InstalledTool;
use std::collections::BTreeMap;
use std::fs;
use serde_json::Value;

/// npm registry doc -> `dist-tags.latest`, the version npx resolves for `@latest`.
pub fn parse_dist_tag_latest(json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json).ok()?;
    v.get("dist-tags")?.get("latest")?.as_str().map(String::from)
}

/// Strip the trailing `@version` from an npx package spec, preserving scoped
/// names. "pkg@1.2.3" -> "pkg"; "@scope/pkg@1.2.3" -> "@scope/pkg".
pub fn npx_pkg_name(spec: &str) -> &str {
    match spec.rfind('@') {
        Some(i) if i > 0 => &spec[..i],
        _ => spec,
    }
}

/// Collapse rows for the same tool (cached in multiple hash dirs), keeping the
/// one with the greatest installed version.
pub fn dedup_npx(rows: Vec<InstalledTool>) -> Vec<InstalledTool> {
    let mut map: BTreeMap<String, InstalledTool> = BTreeMap::new();
    for row in rows {
        map.entry(row.pkg.clone())
            .and_modify(|e| {
                if row.installed > e.installed {
                    *e = row.clone();
                }
            })
            .or_insert(row);
    }
    map.into_values().collect()
}

/// Walk ~/.npm/_npx/<hash>/, read each shim's `_npx.packages` to learn which
/// tool was run, and resolve its version/publisher/description/size from the
/// cached package. `latest` equals `installed` (freshness is unknown for npx
/// until the registry layer). Returns empty if the cache does not exist.
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

    let mut rows: Vec<InstalledTool> = Vec::new();
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
            let pkg_dir = dir.join("node_modules").join(name);
            let v: Value = match fs::read_to_string(pkg_dir.join("package.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
            {
                Some(v) => v,
                None => continue,
            };
            let ver = match v.get("version").and_then(|x| x.as_str()) {
                Some(ver) => ver.to_string(),
                None => continue,
            };
            let publisher = super::publisher::publisher_from_pkg_json(&v).unwrap_or_default();
            let description = v
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            rows.push(InstalledTool {
                name: name.to_string(),
                eco: "npx".to_string(),
                pkg: name.to_string(),
                installed: Some(ver.clone()),
                latest: ver,
                size: super::size::human_size(super::size::dir_size(&pkg_dir)),
                pinned: false,
                publisher,
                description,
                updated: super::path_mtime(&pkg_dir),
                requested: true,
            });
        }
    }
    dedup_npx(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn npx_row(pkg: &str, ver: &str, publisher: &str) -> InstalledTool {
        InstalledTool {
            name: pkg.to_string(),
            eco: "npx".to_string(),
            pkg: pkg.to_string(),
            installed: Some(ver.to_string()),
            latest: ver.to_string(),
            size: String::new(),
            pinned: false,
            publisher: publisher.to_string(),
            description: String::new(),
            updated: 0,
            requested: true,
        }
    }

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
    fn reads_dist_tag_latest() {
        let doc = r#"{"dist-tags":{"latest":"5.6.2","next":"6.0.0-beta"},"name":"typescript"}"#;
        assert_eq!(parse_dist_tag_latest(doc), Some("5.6.2".to_string()));
        assert_eq!(parse_dist_tag_latest("not json"), None);
        assert_eq!(parse_dist_tag_latest(r#"{"dist-tags":{}}"#), None);
    }

    #[test]
    fn dedup_keeps_greatest_version() {
        let rows = dedup_npx(vec![
            npx_row("tool", "1.0.0", "alice"),
            npx_row("tool", "1.2.0", "bob"),
            npx_row("other", "0.1.0", "carol"),
        ]);
        assert_eq!(rows.len(), 2);
        let tool = rows.iter().find(|t| t.pkg == "tool").unwrap();
        assert_eq!(tool.eco, "npx");
        assert_eq!(tool.installed.as_deref(), Some("1.2.0"));
        assert_eq!(tool.publisher, "bob"); // the chosen version's row
    }
}
