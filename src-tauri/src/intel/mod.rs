use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
