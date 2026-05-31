use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod osv;
pub mod wire;
pub mod release;

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
    pub severity: String,            // "malicious" | "vulnerable"
    pub id: String,                  // e.g. "MAL-2024-1" or "GHSA-..."
    pub summary: String,
    pub installed: String,           // the version the user is holding
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
    pub age_label: String,           // "released 6 days ago", or "" when unknown
    pub recommendation: String,      // "safe" | "new" | "unknown"
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
    pub security_ok: bool,           // false => OSV check failed, do not imply clean
    pub wire: Vec<WireItem>,
    pub wire_ok: bool,
    pub verdicts: Vec<ReleaseInfo>,
}

/// Run all three layers concurrently and assemble the feed payload.
/// `verdict_scope` is the list of pkg names (matching ToolRef.pkg) the frontend
/// wants age verdicts for. Verdicts already covered by a security alert are dropped.
pub fn whats_new(installed: &[ToolRef], verdict_scope: &[String], cache_dir: &Path, now: i64) -> WhatsNew {
    std::thread::scope(|s| {
        let sec = s.spawn(|| osv::scan_security(installed));
        let wir = s.spawn(|| wire::fetch_wire(cache_dir));
        let ver = s.spawn(|| {
            verdict_scope.iter().filter_map(|pkg| {
                let t = installed.iter().find(|t| &t.pkg == pkg)?;
                let (rec, age_label) = release::release_age(&t.eco, &t.pkg, &t.latest, now);
                Some(ReleaseInfo {
                    pkg: t.pkg.clone(), eco: t.eco.clone(), version: t.latest.clone(),
                    age_label, recommendation: rec,
                })
            }).collect::<Vec<_>>()
        });

        let sec = sec.join().unwrap_or(None);
        let wir = wir.join().unwrap_or(None);
        let mut verdicts = ver.join().unwrap_or_default();

        let (alerts, security_ok) = match sec {
            Some(a) => (a, true),
            None => (Vec::new(), false),
        };
        // Drop verdicts that are already covered by a security alert.
        let flagged: std::collections::BTreeSet<&str> = alerts.iter().map(|a| a.pkg.as_str()).collect();
        verdicts.retain(|v| !flagged.contains(v.pkg.as_str()));

        let (wire, wire_ok) = match wir {
            Some(w) => (w, true),
            None => (Vec::new(), false),
        };
        WhatsNew { alerts, security_ok, wire, wire_ok, verdicts }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_ref_deserializes_from_frontend_shape() {
        let t: ToolRef = serde_json::from_str(
            r#"{"pkg":"eslint","eco":"npm","installed":"9.0.0","latest":"9.10.0"}"#
        ).unwrap();
        assert_eq!(t.pkg, "eslint");
        assert_eq!(t.eco, "npm");
        assert_eq!(t.installed.as_deref(), Some("9.0.0"));
    }
}
