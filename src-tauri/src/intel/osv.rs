use super::{SecurityAlert, ToolRef};
use serde_json::Value;

/// OSV ecosystem name for our eco string, or None if OSV does not cover it
/// (brew has no Homebrew ecosystem in OSV).
pub fn osv_ecosystem(eco: &str) -> Option<&'static str> {
    match eco {
        "npm" | "npx" => Some("npm"),
        "pip" => Some("PyPI"),
        _ => None,
    }
}

/// Parse an OSV querybatch response into a vector aligned to the request order:
/// each element is the list of advisory IDs affecting that query (empty = clean).
pub fn parse_osv_batch(json: &str) -> Vec<Vec<String>> {
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return Vec::new() };
    let results = match v.get("results").and_then(|r| r.as_array()) {
        Some(a) => a, None => return Vec::new(),
    };
    results.iter().map(|r| {
        r.get("vulns").and_then(|x| x.as_array()).map(|arr| {
            arr.iter().filter_map(|vuln| vuln.get("id").and_then(|i| i.as_str()).map(String::from)).collect()
        }).unwrap_or_default()
    }).collect()
}

/// Classify and summarize a single OSV vuln detail document.
/// Returns (severity, summary, fixed_version). severity is "malicious" for
/// MAL- ids, else "vulnerable".
pub fn parse_osv_vuln(json: &str) -> Option<(String, String, Option<String>)> {
    let v: Value = serde_json::from_str(json).ok()?;
    let id = v.get("id").and_then(|x| x.as_str())?;
    let severity = if id.starts_with("MAL-") { "malicious" } else { "vulnerable" };
    let summary = v.get("summary").and_then(|x| x.as_str())
        .or_else(|| v.get("details").and_then(|x| x.as_str()))
        .unwrap_or("")
        .lines().next().unwrap_or("").trim().to_string();
    // First "fixed" event across all affected ranges.
    let fixed_version = v.get("affected").and_then(|a| a.as_array()).and_then(|affected| {
        affected.iter().find_map(|aff| {
            aff.get("ranges").and_then(|r| r.as_array()).and_then(|ranges| {
                ranges.iter().find_map(|range| {
                    range.get("events").and_then(|e| e.as_array()).and_then(|events| {
                        events.iter().find_map(|ev| ev.get("fixed").and_then(|f| f.as_str()).map(String::from))
                    })
                })
            })
        })
    });
    Some((severity.to_string(), summary, fixed_version))
}

/// Scan installed tools against the OSV batch API.
/// Returns None only if the batch network call itself fails (caller sets security_ok=false).
/// Returns Some(alerts) otherwise (possibly empty).
pub fn scan_security(installed: &[ToolRef]) -> Option<Vec<SecurityAlert>> {
    // Build queries only for tools with a supported ecosystem and an installed version.
    let eligible: Vec<(&ToolRef, &'static str)> = installed.iter().filter_map(|t| {
        let eco = osv_ecosystem(&t.eco)?;
        let _ = t.installed.as_ref()?;
        Some((t, eco))
    }).collect();

    if eligible.is_empty() {
        return Some(Vec::new());
    }

    // Build the batch query body.
    let queries: Vec<serde_json::Value> = eligible.iter().map(|(t, eco)| {
        serde_json::json!({
            "package": { "ecosystem": eco, "name": t.pkg },
            "version": t.installed.as_deref().unwrap_or("")
        })
    }).collect();
    let body = serde_json::json!({ "queries": queries }).to_string();

    let batch_resp = crate::http::post_json("https://api.osv.dev/v1/querybatch", &body).ok()?;
    let id_lists = parse_osv_batch(&batch_resp);

    // A truncated or malformed response (fewer results than queries) would leave
    // tail packages unscanned while still looking successful. Treat any length
    // mismatch as a failed check (None) so it can never be misread as "clean".
    if id_lists.len() != eligible.len() {
        return None;
    }

    // Collect (tool, first_id) pairs for those with advisories.
    let flagged: Vec<(&ToolRef, String)> = eligible.iter().enumerate().filter_map(|(i, (t, _eco))| {
        let ids = id_lists.get(i)?;
        let first = ids.first()?.clone();
        Some((*t, first))
    }).collect();

    // Severity is encoded in the advisory id (MAL- = malicious), so it needs no
    // network call. Build the vulnerable alerts immediately with no detail fetch
    // (these are the bulk; their summary and fixed version load lazily when a
    // card is expanded). Fetch full detail only for the rare malicious alerts, so
    // the remove-vs-fix decision is correct upfront.
    let mut alerts: Vec<SecurityAlert> = Vec::new();
    let mut malicious: Vec<(&ToolRef, String)> = Vec::new();
    for (t, id) in flagged {
        if id.starts_with("MAL-") {
            malicious.push((t, id));
        } else {
            let link = format!("https://osv.dev/vulnerability/{}", id);
            alerts.push(SecurityAlert {
                pkg: t.pkg.clone(), eco: t.eco.clone(), severity: "vulnerable".into(),
                id, summary: String::new(), installed: t.installed.clone().unwrap_or_default(),
                fixed_version: None, link,
            });
        }
    }

    // Malicious are rare; fetch their details in parallel so the card knows
    // upfront whether a fixed version exists or removal is the only remedy.
    std::thread::scope(|s| {
        let handles: Vec<_> = malicious.iter().map(|(t, id)| {
            let id = id.clone();
            let pkg = t.pkg.clone();
            let eco = t.eco.clone();
            let installed_ver = t.installed.clone().unwrap_or_default();
            s.spawn(move || -> SecurityAlert {
                let link = format!("https://osv.dev/vulnerability/{}", id);
                let (summary, fixed_version) = crate::http::get(&format!("https://api.osv.dev/v1/vulns/{}", id))
                    .ok()
                    .and_then(|d| parse_osv_vuln(&d))
                    .map(|(_, sum, fixed)| (sum, fixed))
                    .unwrap_or_default();
                SecurityAlert { pkg, eco, severity: "malicious".into(), id, summary, installed: installed_ver, fixed_version, link }
            })
        }).collect();
        for h in handles {
            if let Ok(alert) = h.join() {
                alerts.push(alert);
            }
        }
    });

    // Sort malicious before vulnerable.
    alerts.sort_by(|a, b| {
        let rank = |s: &str| if s == "malicious" { 0 } else { 1 };
        rank(&a.severity).cmp(&rank(&b.severity))
    });

    Some(alerts)
}

/// Fetch and classify a single advisory by id. Used for lazy detail loading when
/// a vulnerable card is expanded. Returns (severity, summary, fixed_version).
pub fn fetch_advisory(id: &str) -> Option<(String, String, Option<String>)> {
    let detail = crate::http::get(&format!("https://api.osv.dev/v1/vulns/{}", id)).ok()?;
    parse_osv_vuln(&detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_aligns_ids_to_query_order() {
        // 3 queries: clean, vuln, malicious
        let json = r#"{"results":[
            {},
            {"vulns":[{"id":"GHSA-aaaa-bbbb-cccc","modified":"2024-01-01T00:00:00Z"}]},
            {"vulns":[{"id":"MAL-2024-99","modified":"2024-05-01T00:00:00Z"}]}
        ]}"#;
        let ids = parse_osv_batch(json);
        assert_eq!(ids.len(), 3);
        assert!(ids[0].is_empty());
        assert_eq!(ids[1], vec!["GHSA-aaaa-bbbb-cccc".to_string()]);
        assert_eq!(ids[2], vec!["MAL-2024-99".to_string()]);
    }

    #[test]
    fn ecosystem_maps_and_excludes_brew() {
        assert_eq!(osv_ecosystem("npm"), Some("npm"));
        assert_eq!(osv_ecosystem("pip"), Some("PyPI"));
        assert_eq!(osv_ecosystem("brew"), None);
    }

    #[test]
    fn vuln_detail_extracts_severity_summary_fixed() {
        let mal = r#"{"id":"MAL-2024-99","summary":"Malicious code in foo","affected":[]}"#;
        let (sev, sum, fixed) = parse_osv_vuln(mal).unwrap();
        assert_eq!(sev, "malicious");
        assert_eq!(sum, "Malicious code in foo");
        assert_eq!(fixed, None);

        let vuln = r#"{"id":"GHSA-x","details":"Prototype pollution\nmore text",
            "affected":[{"ranges":[{"type":"SEMVER","events":[{"introduced":"0"},{"fixed":"1.2.3"}]}]}]}"#;
        let (sev2, sum2, fixed2) = parse_osv_vuln(vuln).unwrap();
        assert_eq!(sev2, "vulnerable");
        assert_eq!(sum2, "Prototype pollution");
        assert_eq!(fixed2, Some("1.2.3".to_string()));
    }
}
