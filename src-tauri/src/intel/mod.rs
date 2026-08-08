use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod osv;
pub mod registry;
pub mod release;
pub mod wire;

/// The minimal tool identity the frontend sends for each installed tool.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolRef {
    pub pkg: String,
    pub eco: String,
    pub installed: Option<String>,
    pub latest: String,
}

/// A security finding for an installed tool (Layer 1). `severity` is
/// "malicious" (compromise/hijack) or "vulnerable" (CVE/GHSA).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAlert {
    pub pkg: String,
    pub eco: String,
    pub severity: String, // "malicious" | "vulnerable"
    pub id: String,       // e.g. "MAL-2024-1" or "GHSA-..."
    pub summary: String,
    pub installed: String,             // the version the user is holding
    pub fixed_version: Option<String>, // patched version if OSV reports one
    pub link: String,
}

/// One recent ecosystem malware advisory (Layer 2, the wire).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireItem {
    pub id: String,
    pub eco: String,
    pub summary: String,
    pub packages: Vec<String>,
    pub published: String,
    pub link: String,
}

/// An age-based update verdict for an in-scope update (Layer 3).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub pkg: String,
    pub eco: String,
    pub version: String,
    pub age_label: String,      // "released 6 days ago", or "" when unknown
    pub recommendation: String, // "safe" | "new" | "hold" | "unknown"
    pub reason: String,         // hold explanation, "" otherwise
}

/// Lazily-loaded detail for a single advisory (fetched when a card is expanded).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Advisory {
    pub severity: String,
    pub summary: String,
    pub fixed_version: Option<String>,
}

/// Whether the OSV security check actually ran. The frontend must never imply
/// "safe" when the check could not run, so this is explicit.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNew {
    pub alerts: Vec<SecurityAlert>,
    pub security_ok: bool, // false => OSV check failed OR disabled, do not imply clean
    /// True when the advisory scan was skipped by user choice (Preferences),
    /// as opposed to attempted and failing. Distinct from `security_ok` so the
    /// frontend can render "off by choice" rather than "check broke".
    pub security_disabled: bool,
    pub wire: Vec<WireItem>,
    pub wire_ok: bool,
    pub verdicts: Vec<ReleaseInfo>,
}

/// The GitHub token for API calls: the stored settings value if present and
/// non-empty, otherwise the GITHUB_TOKEN env var. None when neither is set.
/// Reads settings.json directly from the cache dir (== app-data dir) to avoid a
/// dependency on the store module.
pub fn github_token(cache_dir: &Path) -> Option<String> {
    let from_file = std::fs::read_to_string(cache_dir.join("settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("githubToken")
                .and_then(|t| t.as_str())
                .map(String::from)
        })
        .filter(|t| !t.trim().is_empty());
    from_file.or_else(|| {
        std::env::var("GITHUB_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty())
    })
}

/// Run all three layers concurrently and assemble the feed payload.
/// `verdict_scope` is the list of pkg names (matching ToolRef.pkg) the frontend
/// wants age verdicts for. Verdicts already covered by a security alert are dropped.
/// `advisories_enabled` gates only the batched OSV inventory scan (the
/// security alerts); the wire and release-age verdicts always run regardless.
pub fn whats_new(
    installed: &[ToolRef],
    verdict_scope: &[String],
    cache_dir: &Path,
    now: i64,
    advisories_enabled: bool,
) -> WhatsNew {
    std::thread::scope(|s| {
        let sec = if advisories_enabled {
            Some(s.spawn(|| osv::scan_security(installed)))
        } else {
            None
        };
        let wir = s.spawn(|| wire::fetch_wire(cache_dir));
        let ver = s.spawn(|| {
            // Resolve the tool refs for each requested pkg first (sequential, cheap).
            let scope_tools: Vec<&ToolRef> = verdict_scope
                .iter()
                .filter_map(|pkg| installed.iter().find(|t| &t.pkg == pkg))
                .collect();
            // Fetch release ages in parallel, bounded at 8 concurrent workers so a
            // user with dozens of outdated tools does not fire one thread per
            // package and thrash the shared HTTP connection pool / GitHub rate
            // limit. Each worker drains its share of indices into a pre-sized
            // slot, then results are flattened to preserve input order.
            let mut results: Vec<Option<ReleaseInfo>> = vec![None; scope_tools.len()];
            std::thread::scope(|inner| {
                let handles: Vec<_> = registry::chunk_indices(scope_tools.len(), 8)
                    .into_iter()
                    .map(|idxs| {
                        let scope_tools = &scope_tools;
                        inner.spawn(move || -> Vec<(usize, ReleaseInfo)> {
                            idxs.into_iter()
                                .map(|i| {
                                    let t = scope_tools[i];
                                    let eco = t.eco.clone();
                                    let pkg = t.pkg.clone();
                                    let ver = t.latest.clone();
                                    let (rec, age_label, reason) =
                                        release::release_verdict(&eco, &pkg, &ver, now, cache_dir);
                                    (
                                        i,
                                        ReleaseInfo {
                                            pkg,
                                            eco,
                                            version: ver,
                                            age_label,
                                            recommendation: rec,
                                            reason,
                                        },
                                    )
                                })
                                .collect()
                        })
                    })
                    .collect();
                for h in handles {
                    if let Ok(pairs) = h.join() {
                        for (i, info) in pairs {
                            results[i] = Some(info);
                        }
                    }
                }
            });
            results.into_iter().flatten().collect::<Vec<_>>()
        });

        let sec = sec.map(|h| h.join().unwrap_or(None));
        let wir = wir.join().unwrap_or(None);
        let mut verdicts = ver.join().unwrap_or_default();

        // `sec` is None when the scan was disabled by user choice (distinct from
        // `Some(None)`, which is an attempted-and-failed OSV call).
        let security_disabled = sec.is_none();
        let (alerts, security_ok) = match sec {
            Some(Some(a)) => (a, true),
            Some(None) => (Vec::new(), false),
            None => (Vec::new(), false),
        };
        // Drop verdicts that are already covered by a security alert. Key on
        // (eco, pkg) so a same-name package in two different ecosystems is not
        // incorrectly suppressed by an alert in only one of them.
        let flagged: std::collections::BTreeSet<(&str, &str)> = alerts
            .iter()
            .map(|a| (a.eco.as_str(), a.pkg.as_str()))
            .collect();
        verdicts.retain(|v| !flagged.contains(&(v.eco.as_str(), v.pkg.as_str())));

        let (wire, wire_ok) = match wir {
            Some((w, complete)) => (w, complete),
            None => (Vec::new(), false),
        };
        WhatsNew {
            alerts,
            security_ok,
            security_disabled,
            wire,
            wire_ok,
            verdicts,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_ref_deserializes_from_frontend_shape() {
        let t: ToolRef = serde_json::from_str(
            r#"{"pkg":"eslint","eco":"npm","installed":"9.0.0","latest":"9.10.0"}"#,
        )
        .unwrap();
        assert_eq!(t.pkg, "eslint");
        assert_eq!(t.eco, "npm");
        assert_eq!(t.installed.as_deref(), Some("9.0.0"));
    }

    #[test]
    fn whats_new_with_advisories_disabled_skips_osv_and_marks_disabled() {
        // The OSV spawn must be skipped entirely (not attempted-and-failed),
        // so the disabled shape is: no alerts, security_ok false, and a
        // distinct security_disabled flag the frontend can render as "off by
        // choice" rather than "check broke". Pre-seed a fresh wire cache so
        // this test never touches the network.
        let mut dir = std::env::temp_dir();
        dir.push(format!("napm-test-whatsnew-disabled-{:p}", &dir));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wire.json"), b"[]").unwrap();

        let installed = vec![ToolRef {
            pkg: "eslint".into(),
            eco: "npm".into(),
            installed: Some("9.0.0".into()),
            latest: "9.0.0".into(),
        }];
        let result = whats_new(&installed, &[], &dir, 0, false);
        assert!(result.alerts.is_empty());
        assert!(!result.security_ok);
        assert!(result.security_disabled);
        assert!(result.wire_ok); // fresh cache hit, unrelated to the advisory toggle

        let _ = std::fs::remove_dir_all(&dir);
    }
}
