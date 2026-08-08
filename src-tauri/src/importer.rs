//! Library import: recreate a toolchain from a manifest exported by
//! `exportLibrary`'s "manifest" flavor (see frontend/index.html).
//!
//! Installable ecosystems as of this build: npm, brew, pip, and cargo. cargo
//! genuinely supports pinning an exact version (see ops::build_command's
//! "cargo install --version"), but v1 does not special-case it: every
//! ecosystem here installs whatever is currently latest, because brew cannot
//! honor a pin at all, and a manifest where most rows respect a pin and one
//! silently cannot is exactly the kind of dishonesty this project avoids.
//! manual and npx remain non-installable and are refused with a reason.

use crate::scan::InstalledTool;
use serde::{Deserialize, Serialize};

const CURRENT_SCHEMA: u32 = 1;
const INSTALLABLE_ECOS: [&str; 4] = ["npm", "brew", "pip", "cargo"];

/// One row in an import manifest: what ecosystem/package/version was recorded
/// at export time. Mirrors the frontend's `{pkg, eco, version}` shape.
#[derive(Debug, Clone, Deserialize)]
pub struct ManifestTool {
    pub pkg: String,
    pub eco: String,
    #[serde(default)]
    pub version: String,
}

/// The versioned import-manifest shape `exportLibrary` writes and
/// `import_preview` reads. `schema` is checked before anything else: an
/// unrecognized value is refused rather than guessed at, so a future
/// incompatible manifest shape fails loudly instead of silently misparsing.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    #[serde(default, rename = "generatedAt")]
    pub generated_at: String,
    pub tools: Vec<ManifestTool>,
}

/// One row in an import preview bucket.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRow {
    pub pkg: String,
    pub eco: String,
    /// The version recorded in the manifest at export time (display only,
    /// e.g. "exported at 1.2.0").
    pub manifest_version: String,
    /// The operative version for this row: for `will_install`, the resolved
    /// install target (falls back to `manifest_version` when the live lookup
    /// fails or hasn't run -- see `resolve_latest_for_will_install`); for
    /// `already_present`, the version currently installed locally; for
    /// `cannot_install`, empty (there is no operative version).
    pub version: String,
    /// Non-empty only in `cannot_install`.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// Echoed from the manifest's `generatedAt`, so the preview modal can
    /// show "exported on ..." next to the buckets. Empty if the manifest
    /// omitted it.
    pub generated_at: String,
    pub will_install: Vec<ImportRow>,
    pub already_present: Vec<ImportRow>,
    pub cannot_install: Vec<ImportRow>,
}

/// Parse + validate a manifest JSON string. Rejects malformed JSON and any
/// `schema` other than the one this build understands.
pub fn parse_manifest(json: &str) -> Result<Manifest, String> {
    let manifest: Manifest =
        serde_json::from_str(json).map_err(|e| format!("not a napm import manifest: {}", e))?;
    if manifest.schema != CURRENT_SCHEMA {
        return Err(format!(
            "unsupported manifest schema {} (this napm understands schema {})",
            manifest.schema, CURRENT_SCHEMA
        ));
    }
    Ok(manifest)
}

/// The reason a manifest row cannot be installed, or None when eco/pkg are
/// well-formed and the ecosystem supports installing.
fn cannot_install_reason(eco: &str, pkg: &str) -> Option<String> {
    if pkg.trim().is_empty() {
        return Some("missing package name".to_string());
    }
    if eco.trim().is_empty() {
        return Some("missing ecosystem".to_string());
    }
    if INSTALLABLE_ECOS.contains(&eco) {
        return None;
    }
    Some(match eco {
        "manual" => "manual tools have no install path".to_string(),
        "npx" => {
            "npx tools are cached on demand, not installed globally; nothing to import".to_string()
        }
        other => format!("unknown ecosystem \"{}\"", other),
    })
}

/// Pure bucket classifier: no I/O, so it is unit-testable without a real
/// filesystem or network. `installed` is a fresh scan (any/all sources),
/// used for an eco-aware (eco, pkg) match -- never a pkg-only lookup, so the
/// same name in two ecosystems (e.g. npm "prettier" and brew "prettier") is
/// judged independently.
pub fn classify(manifest: &Manifest, installed: &[InstalledTool]) -> ImportPreview {
    let mut out = ImportPreview {
        generated_at: manifest.generated_at.clone(),
        ..Default::default()
    };
    for row in &manifest.tools {
        if let Some(reason) = cannot_install_reason(&row.eco, &row.pkg) {
            out.cannot_install.push(ImportRow {
                pkg: row.pkg.clone(),
                eco: row.eco.clone(),
                manifest_version: row.version.clone(),
                version: String::new(),
                reason,
            });
            continue;
        }
        let found = installed
            .iter()
            .find(|t| t.eco == row.eco && t.pkg == row.pkg && t.installed.is_some());
        match found {
            Some(t) => out.already_present.push(ImportRow {
                pkg: row.pkg.clone(),
                eco: row.eco.clone(),
                manifest_version: row.version.clone(),
                version: t.installed.clone().unwrap_or_default(),
                reason: String::new(),
            }),
            None => out.will_install.push(ImportRow {
                pkg: row.pkg.clone(),
                eco: row.eco.clone(),
                manifest_version: row.version.clone(),
                // Fallback until resolve_latest_for_will_install (network,
                // best-effort) can upgrade this to a live latest.
                version: row.version.clone(),
                reason: String::new(),
            }),
        }
    }
    out
}

/// Best-effort upgrade of each `will_install` row's `version` from the
/// manifest-recorded fallback to the ecosystem's current latest, reusing the
/// same registry lookups Search and the npx-drift check already use. A row
/// whose lookup fails (offline, unknown package, dead registry) keeps its
/// manifest version as an honest fallback rather than blocking the import --
/// mirroring the fail-open pattern the rest of the app uses for best-effort
/// network checks (e.g. `ops::brew_dependents`).
pub fn resolve_latest_for_will_install(preview: &mut ImportPreview, cache_dir: &std::path::Path) {
    for row in preview.will_install.iter_mut() {
        if let Some(latest) = resolve_latest(&row.eco, &row.pkg, cache_dir) {
            row.version = latest;
        }
    }
}

fn resolve_latest(eco: &str, pkg: &str, cache_dir: &std::path::Path) -> Option<String> {
    match eco {
        "npm" => {
            let body = crate::intel::registry::doc("npm", pkg, cache_dir)?;
            crate::scan::npx::parse_dist_tag_latest(&body)
        }
        "pip" => {
            let body = crate::intel::registry::doc("pip", pkg, cache_dir)?;
            crate::search::pip::parse_pypi(&body)
                .map(|r| r.version)
                .filter(|v| !v.is_empty())
        }
        "cargo" => {
            let body = crate::intel::registry::doc("cargo", pkg, cache_dir)?;
            crate::scan::cargo::parse_crates_io_latest(&body)
        }
        // crates.io/npm/pypi all expose a single-package JSON document this
        // cache understands (see intel::registry::url_for); brew has none, so
        // it falls back to an exact match against the cached formula catalog
        // the way Search already does.
        "brew" => crate::search::brew::search_brew(pkg, cache_dir)
            .into_iter()
            .find(|r| r.pkg.eq_ignore_ascii_case(pkg))
            .map(|r| r.version)
            .filter(|v| !v.is_empty()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(eco: &str, pkg: &str, installed: Option<&str>) -> InstalledTool {
        InstalledTool {
            name: pkg.to_string(),
            eco: eco.to_string(),
            pkg: pkg.to_string(),
            installed: installed.map(|s| s.to_string()),
            latest: installed.unwrap_or("").to_string(),
            size: String::new(),
            pinned: false,
            publisher: String::new(),
            description: String::new(),
            updated: 0,
            requested: true,
            status: String::new(),
            bump: String::new(),
        }
    }

    fn manifest(tools: Vec<(&str, &str, &str)>) -> Manifest {
        Manifest {
            schema: 1,
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            tools: tools
                .into_iter()
                .map(|(pkg, eco, version)| ManifestTool {
                    pkg: pkg.to_string(),
                    eco: eco.to_string(),
                    version: version.to_string(),
                })
                .collect(),
        }
    }

    // ---- parse_manifest ----------------------------------------------

    #[test]
    fn parses_a_well_formed_manifest() {
        let json = r#"{"schema":1,"generatedAt":"2026-01-01T00:00:00Z","tools":[{"pkg":"ripgrep","eco":"brew","version":"14.1.1"}]}"#;
        let m = parse_manifest(json).unwrap();
        assert_eq!(m.schema, 1);
        assert_eq!(m.tools.len(), 1);
        assert_eq!(m.tools[0].pkg, "ripgrep");
    }

    #[test]
    fn rejects_an_unrecognized_schema() {
        let json = r#"{"schema":2,"generatedAt":"","tools":[]}"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(
            err.contains("schema"),
            "error should mention schema: {}",
            err
        );
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_manifest("not json").is_err());
        assert!(parse_manifest("{}").is_err()); // missing required fields
    }

    // ---- classify ------------------------------------------------------

    #[test]
    fn already_present_is_eco_aware_not_pkg_only() {
        // "prettier" installed via npm only. The manifest asks for
        // "prettier" in BOTH npm and brew: npm must land in already_present,
        // brew must land in will_install -- a pkg-only match would wrongly
        // mark both as already present.
        let installed = vec![tool("npm", "prettier", Some("3.0.0"))];
        let m = manifest(vec![
            ("prettier", "npm", "3.0.0"),
            ("prettier", "brew", "3.0.0"),
        ]);
        let preview = classify(&m, &installed);
        assert_eq!(preview.already_present.len(), 1);
        assert_eq!(preview.already_present[0].eco, "npm");
        assert_eq!(preview.will_install.len(), 1);
        assert_eq!(preview.will_install[0].eco, "brew");
    }

    #[test]
    fn already_present_uses_the_locally_installed_version_not_the_manifest_version() {
        let installed = vec![tool("pip", "httpie", Some("3.9.0"))];
        let m = manifest(vec![("httpie", "pip", "3.2.2")]);
        let preview = classify(&m, &installed);
        assert_eq!(preview.already_present.len(), 1);
        assert_eq!(preview.already_present[0].version, "3.9.0");
        assert_eq!(preview.already_present[0].manifest_version, "3.2.2");
    }

    #[test]
    fn not_locally_installed_lands_in_will_install() {
        let m = manifest(vec![("cowsay", "npm", "1.6.0")]);
        let preview = classify(&m, &[]);
        assert_eq!(preview.will_install.len(), 1);
        assert_eq!(preview.will_install[0].pkg, "cowsay");
        // Before network resolution runs, the fallback is the manifest's own version.
        assert_eq!(preview.will_install[0].version, "1.6.0");
    }

    #[test]
    fn manual_and_npx_are_cannot_install_with_a_reason() {
        let m = manifest(vec![
            ("some-script", "manual", "1.0.0"),
            ("create-vite", "npx", "5.0.0"),
        ]);
        let preview = classify(&m, &[]);
        assert_eq!(preview.cannot_install.len(), 2);
        assert!(preview.will_install.is_empty());
        assert!(preview.cannot_install.iter().all(|r| !r.reason.is_empty()));
        assert!(preview.cannot_install[0].reason.contains("manual"));
        assert!(preview.cannot_install[1].reason.contains("npx"));
    }

    #[test]
    fn unknown_ecosystem_is_cannot_install_with_a_reason() {
        let m = manifest(vec![("mystery", "conda", "1.0.0")]);
        let preview = classify(&m, &[]);
        assert_eq!(preview.cannot_install.len(), 1);
        assert!(preview.cannot_install[0].reason.contains("conda"));
    }

    #[test]
    fn malformed_row_with_empty_package_name_is_cannot_install() {
        let m = manifest(vec![("", "npm", "1.0.0")]);
        let preview = classify(&m, &[]);
        assert_eq!(preview.cannot_install.len(), 1);
        assert!(preview.cannot_install[0].reason.contains("package name"));
    }

    #[test]
    fn malformed_row_with_empty_ecosystem_is_cannot_install() {
        let m = manifest(vec![("something", "", "1.0.0")]);
        let preview = classify(&m, &[]);
        assert_eq!(preview.cannot_install.len(), 1);
        assert!(preview.cannot_install[0].reason.contains("ecosystem"));
    }

    #[test]
    fn classify_echoes_generated_at_from_the_manifest() {
        let m = manifest(vec![]);
        let preview = classify(&m, &[]);
        assert_eq!(preview.generated_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn all_four_installable_ecosystems_are_recognized() {
        let m = manifest(vec![
            ("a", "npm", "1.0.0"),
            ("b", "brew", "1.0.0"),
            ("c", "pip", "1.0.0"),
            ("d", "cargo", "1.0.0"),
        ]);
        let preview = classify(&m, &[]);
        assert_eq!(preview.will_install.len(), 4);
        assert!(preview.cannot_install.is_empty());
    }
}
