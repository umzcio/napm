//! Pure helpers for deriving a real "publisher" handle from local package
//! metadata. The "Shared By" column renders `@<publisher>`; an empty string
//! means no author metadata was found and the UI shows a neutral @unknown.

use serde_json::Value;

/// Normalize an author display name into a lowercase handle: lowercase, map any
/// run of non-alphanumeric characters to a single hyphen, trim hyphens. Returns
/// None if nothing usable remains. "Andrew Gallant" -> "andrew-gallant",
/// "Anthropic, PBC" -> "anthropic-pbc", "BurntSushi" -> "burntsushi".
pub fn to_handle(name: &str) -> Option<String> {
    let mut out = String::new();
    let mut prev_hyphen = true; // leading: suppress hyphens
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            out.push('-');
            prev_hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// A usable author name is non-empty, not a URL, and not a bare email (some
/// packages stuff a contributors link or a raw address into the author field).
fn usable_name(name: &str) -> bool {
    !name.is_empty() && !name.contains("://") && !name.contains('@')
}

/// Best author handle source from a pip dist-info METADATA body: prefer the
/// `Author:` line, then fall back to the name portion of `Author-email:`
/// (modern packaging often leaves `Author` blank and uses `Name <email>`).
pub fn pip_author(metadata: &str) -> Option<String> {
    author_from_metadata(metadata).or_else(|| {
        metadata_field(metadata, "Author-email").and_then(|s| author_name_from_string(&s))
    })
}

/// Extract the display name from an npm-style author string, dropping the
/// `<email>` and `(url)` parts. "Sindre Sorhus <s@x.com> (https://x.com)"
/// -> "Sindre Sorhus".
pub fn author_name_from_string(s: &str) -> Option<String> {
    let name = s.split(['<', '(']).next().unwrap_or("").trim();
    if usable_name(name) {
        Some(name.to_string())
    } else {
        None
    }
}

/// Extract an author display name from a package.json `author` field, which may
/// be a string ("Name <email>") or an object ({"name": "..."}).
pub fn author_from_pkg_json(author: &Value) -> Option<String> {
    match author {
        Value::String(s) => author_name_from_string(s),
        Value::Object(_) => author
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| usable_name(s)),
        _ => None,
    }
}

/// Derive a publisher from a project homepage URL. Prefers the GitHub/GitLab
/// owner ("https://github.com/BurntSushi/ripgrep" -> "BurntSushi"); otherwise
/// falls back to the second-level domain label ("https://www.openssl.org/"
/// -> "openssl"). Returns None when no host is present.
pub fn publisher_from_homepage(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    for host in ["github.com/", "gitlab.com/"] {
        if let Some(idx) = lower.find(host) {
            let rest = &url[idx + host.len()..];
            let owner = rest.split('/').next().unwrap_or("").trim();
            if !owner.is_empty() {
                return Some(owner.to_string());
            }
        }
    }
    domain_label(url)
}

/// The second-level domain label of a URL's host: "https://www.openssl.org/x"
/// -> "openssl", "https://gnu.org/" -> "gnu". None if there is no dotted host.
fn domain_label(url: &str) -> Option<String> {
    let after = url.split("://").nth(1).unwrap_or(url);
    let host = after.split('/').next().unwrap_or("").trim_start_matches("www.");
    let parts: Vec<&str> = host.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 2 {
        Some(parts[parts.len() - 2].to_string())
    } else {
        None
    }
}

/// Read a single `Field: value` line from a pip dist-info METADATA body.
/// Returns None when the field is absent or its value is empty.
pub fn metadata_field(metadata: &str, field: &str) -> Option<String> {
    let prefix = format!("{}:", field);
    for line in metadata.lines() {
        if let Some(rest) = line.strip_prefix(prefix.as_str()) {
            let v = rest.trim();
            return if v.is_empty() { None } else { Some(v.to_string()) };
        }
    }
    None
}

/// Extract the `Author:` value from a pip dist-info METADATA file body.
/// Returns None when absent, empty, or the literal "UNKNOWN".
pub fn author_from_metadata(metadata: &str) -> Option<String> {
    metadata_field(metadata, "Author").filter(|v| !v.eq_ignore_ascii_case("UNKNOWN"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn to_handle_normalizes() {
        assert_eq!(to_handle("BurntSushi").as_deref(), Some("burntsushi"));
        assert_eq!(to_handle("Andrew Gallant").as_deref(), Some("andrew-gallant"));
        assert_eq!(to_handle("Anthropic, PBC").as_deref(), Some("anthropic-pbc"));
        assert_eq!(to_handle("  "), None);
        assert_eq!(to_handle(""), None);
        assert_eq!(to_handle("...!!!"), None);
    }

    #[test]
    fn author_name_from_string_drops_email_and_url() {
        assert_eq!(
            author_name_from_string("Sindre Sorhus <sindre@x.com> (https://x.com)").as_deref(),
            Some("Sindre Sorhus")
        );
        assert_eq!(author_name_from_string("Plain Name").as_deref(), Some("Plain Name"));
        assert_eq!(author_name_from_string("<only@email>"), None);
        // a URL stuffed into the author field is not a usable name
        assert_eq!(author_name_from_string("https://github.com/foo/bar/graphs/contributors"), None);
    }

    #[test]
    fn author_from_pkg_json_handles_string_and_object() {
        assert_eq!(
            author_from_pkg_json(&json!("Anthropic <s@a.com>")).as_deref(),
            Some("Anthropic")
        );
        assert_eq!(
            author_from_pkg_json(&json!({"name": "Google LLC"})).as_deref(),
            Some("Google LLC")
        );
        assert_eq!(author_from_pkg_json(&json!(42)), None);
        assert_eq!(author_from_pkg_json(&json!({"email": "x@y.com"})), None);
    }

    #[test]
    fn publisher_from_homepage_extracts_owner() {
        assert_eq!(
            publisher_from_homepage("https://github.com/BurntSushi/ripgrep").as_deref(),
            Some("BurntSushi")
        );
        assert_eq!(
            publisher_from_homepage("https://gitlab.com/foo/bar").as_deref(),
            Some("foo")
        );
        // non-git homepage falls back to the second-level domain label
        assert_eq!(
            publisher_from_homepage("https://www.openssl.org/").as_deref(),
            Some("openssl")
        );
        assert_eq!(publisher_from_homepage("https://gnu.org/software/wget/").as_deref(), Some("gnu"));
        assert_eq!(publisher_from_homepage("not a url"), None);
    }

    #[test]
    fn author_from_metadata_parses_author_line() {
        let md = "Metadata-Version: 2.1\nName: Flask\nAuthor: Pallets\nVersion: 3.0.0\n";
        assert_eq!(author_from_metadata(md).as_deref(), Some("Pallets"));
        assert_eq!(author_from_metadata("Author: UNKNOWN\n"), None);
        assert_eq!(author_from_metadata("Author:   \n"), None);
        assert_eq!(author_from_metadata("Name: x\n"), None);
    }

    #[test]
    fn pip_author_falls_back_to_author_email_name() {
        // modern packaging: Author blank, name lives in Author-email
        let md = "Name: ruff\nAuthor-email: Charlie Marsh <charlie@astral.sh>\n";
        assert_eq!(pip_author(md).as_deref(), Some("Charlie Marsh"));
        // a bare email with no name is not a usable handle
        let md2 = "Author-email: noreply@example.com\n";
        assert_eq!(pip_author(md2), None);
        // explicit Author wins over Author-email
        let md3 = "Author: Pallets\nAuthor-email: someone <x@y.com>\n";
        assert_eq!(pip_author(md3).as_deref(), Some("Pallets"));
    }

    #[test]
    fn metadata_field_reads_named_field() {
        let md = "Name: Flask\nAuthor: Pallets\n";
        assert_eq!(metadata_field(md, "Name").as_deref(), Some("Flask"));
        assert_eq!(metadata_field(md, "Author").as_deref(), Some("Pallets"));
        assert_eq!(metadata_field(md, "Missing"), None);
    }

    #[test]
    fn end_to_end_homepage_to_handle() {
        let owner = publisher_from_homepage("https://github.com/BurntSushi/ripgrep").unwrap();
        assert_eq!(to_handle(&owner).as_deref(), Some("burntsushi"));
    }
}
