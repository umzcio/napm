//! `cargo install`ed binaries as a scanned ecosystem.
//!
//! Primary source: `<cargo-root>/.crates2.json`, a structured JSON file cargo
//! itself maintains (name, version, source, and binary names per install).
//! Fallback: `cargo install --list`, cargo's own text summary, used only when
//! the primary file is missing or fails to parse as JSON.
//!
//! Unlike npm/brew/pip, cargo has no batch "outdated" command, so "latest" is
//! resolved per registry-sourced crate against crates.io through the shared
//! registry-document cache (see `resolve_latest`).

use super::InstalledTool;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The source flavor recorded in a `.crates2.json` install key's parenthesized
/// suffix, e.g. `"ripgrep 14.1.1 (registry+https://github.com/rust-lang/crates.io-index)"`.
/// Deliberately not an `InstalledTool` field: it only matters at parse time, to
/// decide whether crates.io has a "latest" for this row at all.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CrateSource {
    /// A crates.io (or other registry) install: crates.io has a "latest".
    Registry,
    /// A `cargo install --git`: no registry version to compare against.
    Git,
    /// A `cargo install --path`: likewise no registry version.
    Path,
    /// Some other/unrecognized source annotation: treated like Git/Path, i.e.
    /// no registry lookup, rather than guessing.
    Other,
}

/// One parsed cargo install, before it becomes an `InstalledTool` row.
#[derive(Debug, Clone, PartialEq)]
struct CargoInstall {
    name: String,
    version: String,
    source: CrateSource,
    /// Binary names this crate installed (e.g. `["rg"]`), from `.crates2.json`'s
    /// `bins` array, or (for the `--list` fallback) the indented lines under
    /// the crate's header. Used by the manual scanner to exclude every binary
    /// a multi-binary crate installs, not just the crate name itself.
    bins: Vec<String>,
}

fn classify_source(suffix: &str) -> CrateSource {
    if suffix.starts_with("registry+") {
        CrateSource::Registry
    } else if suffix.starts_with("git+") {
        CrateSource::Git
    } else if suffix.starts_with("path+") {
        CrateSource::Path
    } else {
        CrateSource::Other
    }
}

/// Parse one `.crates2.json` install key: `"<name> <version> (<source>)"`.
fn parse_package_id(key: &str) -> Option<(String, String, CrateSource)> {
    let open = key.rfind(" (")?;
    if !key.ends_with(')') {
        return None;
    }
    let head = &key[..open];
    let suffix = &key[open + 2..key.len() - 1];
    let mut parts = head.splitn(2, ' ');
    let name = parts.next()?.trim();
    let version = parts.next()?.trim();
    if name.is_empty() || version.is_empty() {
        return None;
    }
    Some((
        name.to_string(),
        version.to_string(),
        classify_source(suffix),
    ))
}

/// Parse `.crates2.json`'s `installs` object into `CargoInstall`s. Empty on
/// missing/malformed JSON or a missing/malformed `installs` object, never panics.
fn parse_crates2(json: &str) -> Vec<CargoInstall> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let installs = match v.get("installs").and_then(|i| i.as_object()) {
        Some(o) => o,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for (key, info) in installs {
        let (name, version, source) = match parse_package_id(key) {
            Some(t) => t,
            None => continue,
        };
        let bins: Vec<String> = info
            .get("bins")
            .and_then(|b| b.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        out.push(CargoInstall {
            name,
            version,
            source,
            bins,
        });
    }
    out
}

/// Parse `cargo install --list` text output (the fallback when `.crates2.json`
/// is missing or unparsable). Each install is a header line
/// (`"<name> v<version>:"`, optionally with a `(<source>)` annotation before
/// the colon) followed by indented binary-name lines. Best-effort: an
/// unrecognized header line is skipped rather than aborting the whole parse.
fn parse_install_list(text: &str) -> Vec<CargoInstall> {
    let mut out = Vec::new();
    let mut current: Option<CargoInstall> = None;
    for line in text.lines() {
        if line.starts_with(char::is_whitespace) {
            if let Some(cur) = current.as_mut() {
                let bin = line.trim();
                if !bin.is_empty() {
                    cur.bins.push(bin.to_string());
                }
            }
            continue;
        }
        if let Some(cur) = current.take() {
            out.push(cur);
        }
        let header = line.trim().trim_end_matches(':');
        if header.is_empty() {
            continue;
        }
        current = parse_list_header(header);
    }
    if let Some(cur) = current.take() {
        out.push(cur);
    }
    out
}

/// Parse one `cargo install --list` header, e.g. `"ripgrep v14.1.1"` or
/// `"my-tool v0.1.0 (https://github.com/user/repo#sha)"`. The `--list` format
/// carries a bare URL/path rather than the `.crates2.json` key's
/// `registry+`/`git+`/`path+`-prefixed source, so the source is classified
/// heuristically here (fallback only; `.crates2.json` is primary).
fn parse_list_header(header: &str) -> Option<CargoInstall> {
    let (head, source_str) = match header.rfind('(') {
        Some(i) if header.ends_with(')') => {
            (header[..i].trim(), Some(&header[i + 1..header.len() - 1]))
        }
        _ => (header, None),
    };
    let mut parts = head.splitn(2, ' ');
    let name = parts.next()?.trim();
    let ver_token = parts.next()?.trim();
    let version = ver_token.strip_prefix('v').unwrap_or(ver_token);
    if name.is_empty() || version.is_empty() {
        return None;
    }
    let source = match source_str {
        None => CrateSource::Registry,
        Some(s) if s.starts_with('/') || s.starts_with("file:") => CrateSource::Path,
        Some(s) if s.contains("://") => CrateSource::Git,
        Some(_) => CrateSource::Other,
    };
    Some(CargoInstall {
        name: name.to_string(),
        version: version.to_string(),
        source,
        bins: Vec::new(),
    })
}

/// Extract `root = "..."` from the `[install]` table of a cargo config file.
/// A minimal line scan rather than a full TOML parser: napm has no TOML
/// dependency, and this is the one key it needs. `#`-comments and quoting
/// with `'` or `"` are handled; anything more exotic (multi-line strings,
/// inline tables) is not expected here and simply yields None.
fn parse_install_root(toml_text: &str) -> Option<String> {
    let mut in_install = false;
    for raw in toml_text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(section) = line.strip_prefix('[') {
            in_install = section.trim_end_matches(']').trim() == "install";
            continue;
        }
        if !in_install {
            continue;
        }
        if let Some(rest) = line.strip_prefix("root") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let val = rest.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

fn nonempty_env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Resolve the cargo install root: `CARGO_INSTALL_ROOT` env var, else the
/// `install.root` key in the cargo config, else `CARGO_HOME`, else
/// `$HOME/.cargo`. None only when no home/env signal exists at all.
fn resolve_cargo_root() -> Option<PathBuf> {
    if let Some(p) = nonempty_env_path("CARGO_INSTALL_ROOT") {
        return Some(p);
    }
    let cargo_home = nonempty_env_path("CARGO_HOME")
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
    if let Some(ch) = &cargo_home {
        for name in ["config.toml", "config"] {
            if let Ok(text) = std::fs::read_to_string(ch.join(name)) {
                if let Some(root) = parse_install_root(&text) {
                    return Some(PathBuf::from(root));
                }
            }
        }
    }
    cargo_home
}

/// Memoized cargo install root (mirrors `npm::npm_root`/`pip::pip_bin`). A
/// change to `CARGO_INSTALL_ROOT`/the cargo config while napm is running is
/// only picked up after an app restart.
pub(crate) fn cargo_root() -> &'static Option<PathBuf> {
    static P: OnceLock<Option<PathBuf>> = OnceLock::new();
    P.get_or_init(resolve_cargo_root)
}

/// crates.io crate JSON (`GET /api/v1/crates/<name>`) -> the latest version.
/// Prefers `max_stable_version` (skips yanked/prerelease-only states some
/// crates hit), falling back to `max_version`.
pub fn parse_crates_io_latest(json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json).ok()?;
    let c = v.get("crate")?;
    c.get("max_stable_version")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| c.get("max_version").and_then(|x| x.as_str()))
        .map(String::from)
}

/// Minimal `Cargo.toml` `[package]` `description`/first `authors` entry
/// extraction, mirroring `parse_install_root`'s line-scan approach (no TOML
/// dependency). Only single-line string values are handled, which covers the
/// overwhelming majority of published crates.
fn parse_cargo_toml_meta(toml_text: &str) -> (Option<String>, Option<String>) {
    let mut in_package = false;
    let mut description = None;
    let mut author = None;
    for raw in toml_text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_package = line.trim_start_matches('[').trim_end_matches(']').trim() == "package";
            continue;
        }
        if !in_package {
            continue;
        }
        if description.is_none() {
            if let Some(rest) = line.strip_prefix("description") {
                if let Some(val) = single_line_string_value(rest) {
                    description = Some(val);
                }
            }
        }
        if author.is_none() {
            if let Some(rest) = line.strip_prefix("authors") {
                if let Some(val) = first_array_string(rest) {
                    author = Some(val);
                }
            }
        }
    }
    (description, author)
}

/// `= "value"` (or `= 'value'`) -> `Some("value")`, after an `=`. None if
/// there is no `=` or the remainder is not a simple quoted string (e.g. it
/// spans multiple lines).
fn single_line_string_value(rest: &str) -> Option<String> {
    let rest = rest.trim_start().strip_prefix('=')?.trim();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let body = &rest[1..];
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

/// `= ["First Author <e@x.com>", "Second"]` -> `Some("First Author <e@x.com>")`.
fn first_array_string(rest: &str) -> Option<String> {
    let rest = rest.trim_start().strip_prefix('=')?.trim();
    let rest = rest.strip_prefix('[')?;
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let body = &rest[1..];
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

fn to_installed_tool(c: &CargoInstall, root: &Path) -> InstalledTool {
    // Registry crates start with an empty `latest` sentinel, resolved by
    // `enrich` against crates.io. git/path crates have no registry "latest":
    // latest == installed makes `version::status_of` read "current" and the
    // frontend show no Update action, which is the honest state for a source
    // napm cannot compare against anything (mirrors npx's "freshness unknown"
    // latest == installed pattern).
    let latest = match c.source {
        CrateSource::Registry => String::new(),
        CrateSource::Git | CrateSource::Path | CrateSource::Other => c.version.clone(),
    };
    let bin_dir = root.join("bin");
    let bins: Vec<&str> = if c.bins.is_empty() {
        vec![c.name.as_str()]
    } else {
        c.bins.iter().map(|s| s.as_str()).collect()
    };
    let size: u64 = bins
        .iter()
        .map(|b| super::size::dir_size(&bin_dir.join(b)))
        .sum();
    let updated = bins
        .iter()
        .map(|b| super::path_mtime(&bin_dir.join(b)))
        .max()
        .unwrap_or(0);

    InstalledTool {
        name: c.name.clone(),
        eco: "cargo".to_string(),
        pkg: c.name.clone(),
        installed: Some(c.version.clone()),
        latest,
        size: super::size::human_size(size),
        pinned: false,
        publisher: String::new(),
        description: String::new(),
        updated,
        requested: true,
        status: String::new(),
        bump: String::new(),
    }
}

/// Every crate's binary names, for the manual scanner's exclusion set (decision
/// 5: a multi-binary crate must exclude ALL its binaries, not just the crate
/// name). Falls back to the crate name itself when `bins` is empty (e.g. the
/// `--list` fallback parse for a crate with no indented lines).
fn all_bin_names(installs: &[CargoInstall]) -> Vec<String> {
    installs
        .iter()
        .flat_map(|c| {
            if c.bins.is_empty() {
                vec![c.name.clone()]
            } else {
                c.bins.clone()
            }
        })
        .collect()
}

/// Resolve "latest" for every registry-sourced row (empty `latest` sentinel)
/// against crates.io, through the shared registry-document cache. Bounded at 8
/// concurrent workers, mirroring `lib.rs::npx_latest`. A failed/unresolved
/// lookup leaves `latest == installed`, matching the "never claim an update
/// exists when unsure" rule everywhere else in napm.
fn resolve_latest(rows: &mut [InstalledTool], cache_dir: &Path) {
    let idxs: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.latest.is_empty())
        .map(|(i, _)| i)
        .collect();
    if idxs.is_empty() {
        return;
    }
    let pkgs: Vec<String> = idxs.iter().map(|&i| rows[i].pkg.clone()).collect();
    let mut results: Vec<Option<String>> = vec![None; idxs.len()];
    std::thread::scope(|s| {
        let handles: Vec<_> = crate::intel::registry::chunk_indices(idxs.len(), 8)
            .into_iter()
            .map(|chunk| {
                let pkgs = &pkgs;
                s.spawn(move || -> Vec<(usize, Option<String>)> {
                    chunk
                        .into_iter()
                        .map(|j| {
                            let latest = crate::intel::registry::doc("cargo", &pkgs[j], cache_dir)
                                .as_deref()
                                .and_then(parse_crates_io_latest);
                            (j, latest)
                        })
                        .collect()
                })
            })
            .collect();
        for h in handles {
            if let Ok(pairs) = h.join() {
                for (j, latest) in pairs {
                    results[j] = latest;
                }
            }
        }
    });
    for (j, latest) in results.into_iter().enumerate() {
        let row = &mut rows[idxs[j]];
        row.latest = latest.unwrap_or_else(|| row.installed.clone().unwrap_or_default());
    }
}

/// Read a crate's `Cargo.toml` from cargo's unpacked registry source cache
/// (`<cargo-home>/registry/src/<index-dir>/<name>-<version>/Cargo.toml`) and
/// fill publisher/description. Best-effort: many `<index-dir>` shards can
/// exist (one per configured registry), so every one is checked.
fn enrich_metadata(rows: &mut [InstalledTool], cargo_home: &Path) {
    let src_root = cargo_home.join("registry").join("src");
    let index_dirs = match std::fs::read_dir(&src_root) {
        Ok(e) => e.flatten().map(|e| e.path()).collect::<Vec<_>>(),
        Err(_) => return,
    };
    for row in rows.iter_mut() {
        let installed = match &row.installed {
            Some(v) if !v.is_empty() => v,
            _ => continue,
        };
        let crate_dir_name = format!("{}-{}", row.pkg, installed);
        for index_dir in &index_dirs {
            let manifest = index_dir.join(&crate_dir_name).join("Cargo.toml");
            if let Ok(text) = std::fs::read_to_string(&manifest) {
                let (description, author) = parse_cargo_toml_meta(&text);
                if let Some(d) = description {
                    row.description = d;
                }
                if let Some(a) = author {
                    if let Some(name) = super::publisher::author_name_from_string(&a) {
                        if let Some(h) = super::publisher::to_handle(&name) {
                            row.publisher = h;
                        }
                    }
                }
                break;
            }
        }
    }
}

/// Scan `cargo install`ed binaries: `.crates2.json` primary, `cargo install
/// --list` fallback (only when the primary file is missing or fails to parse
/// as JSON at all -- a validly-parsed-but-empty file is trusted as "no
/// installs", not treated as a reason to fall back). Empty when no cargo root
/// can be resolved.
///
/// Returns `(rows, all_bin_names)`: the second element is every binary name
/// across every parsed install (decision 5), which `scan::scan_all` folds
/// into the manual scanner's exclusion set so a multi-binary crate's *other*
/// binaries (e.g. `cargo-edit`'s `cargo-add`/`cargo-rm`/...) are excluded too,
/// not just the crate's own row name.
pub fn scan_cargo_with_bins(cache_dir: &Path) -> (Vec<InstalledTool>, Vec<String>) {
    let root = match cargo_root() {
        Some(p) => p.clone(),
        None => return (Vec::new(), Vec::new()),
    };
    let text = std::fs::read_to_string(root.join(".crates2.json")).unwrap_or_default();
    let installs = if serde_json::from_str::<Value>(&text).is_ok() {
        parse_crates2(&text)
    } else {
        parse_install_list(&super::run("cargo", &["install", "--list"]))
    };

    let bins = all_bin_names(&installs);
    let mut rows: Vec<InstalledTool> = installs
        .iter()
        .map(|c| to_installed_tool(c, &root))
        .collect();
    resolve_latest(&mut rows, cache_dir);
    enrich_metadata(&mut rows, &root);
    (rows, bins)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_crates2() -> &'static str {
        r#"{
            "v": 1,
            "installs": {
                "ripgrep 14.1.1 (registry+https://github.com/rust-lang/crates.io-index)": {
                    "version_req": null,
                    "bins": ["rg"],
                    "features": [],
                    "profile": "release",
                    "target": "aarch64-apple-darwin",
                    "rustc": "1.78.0"
                },
                "cargo-edit 0.12.2 (registry+https://github.com/rust-lang/crates.io-index)": {
                    "version_req": null,
                    "bins": ["cargo-add", "cargo-rm", "cargo-set-version", "cargo-upgrade"],
                    "features": [],
                    "profile": "release",
                    "target": "aarch64-apple-darwin",
                    "rustc": "1.78.0"
                },
                "my-tool 0.1.0 (git+https://github.com/user/repo#abc123def456)": {
                    "version_req": null,
                    "bins": ["my-tool"],
                    "features": [],
                    "profile": "release",
                    "target": "aarch64-apple-darwin",
                    "rustc": "1.78.0"
                },
                "local-tool 0.2.0 (path+file:///Users/me/projects/local-tool)": {
                    "version_req": null,
                    "bins": ["local-tool"],
                    "features": [],
                    "profile": "release",
                    "target": "aarch64-apple-darwin",
                    "rustc": "1.78.0"
                }
            }
        }"#
    }

    #[test]
    fn parses_registry_crate_with_single_binary() {
        let installs = parse_crates2(fixture_crates2());
        let rg = installs.iter().find(|c| c.name == "ripgrep").unwrap();
        assert_eq!(rg.version, "14.1.1");
        assert_eq!(rg.source, CrateSource::Registry);
        assert_eq!(rg.bins, vec!["rg".to_string()]);
    }

    #[test]
    fn parses_multi_binary_crate() {
        let installs = parse_crates2(fixture_crates2());
        let edit = installs.iter().find(|c| c.name == "cargo-edit").unwrap();
        assert_eq!(edit.bins.len(), 4);
        assert!(edit.bins.contains(&"cargo-add".to_string()));
        assert!(edit.bins.contains(&"cargo-upgrade".to_string()));
    }

    #[test]
    fn classifies_git_and_path_sources() {
        let installs = parse_crates2(fixture_crates2());
        let git = installs.iter().find(|c| c.name == "my-tool").unwrap();
        assert_eq!(git.source, CrateSource::Git);
        let path = installs.iter().find(|c| c.name == "local-tool").unwrap();
        assert_eq!(path.source, CrateSource::Path);
    }

    #[test]
    fn empty_or_garbage_output_yields_no_rows() {
        assert!(parse_crates2("").is_empty());
        assert!(parse_crates2("not json").is_empty());
        assert!(parse_crates2(r#"{"v":1}"#).is_empty()); // no "installs" key
        assert!(parse_crates2(r#"{"v":1,"installs":{}}"#).is_empty());
    }

    #[test]
    fn to_installed_tool_registry_row_has_empty_latest_sentinel() {
        let installs = parse_crates2(fixture_crates2());
        let rg = installs.iter().find(|c| c.name == "ripgrep").unwrap();
        let row = to_installed_tool(rg, Path::new("/Users/x/.cargo"));
        assert_eq!(row.eco, "cargo");
        assert_eq!(row.pkg, "ripgrep");
        assert_eq!(row.installed.as_deref(), Some("14.1.1"));
        assert_eq!(row.latest, ""); // resolved later by resolve_latest
    }

    #[test]
    fn to_installed_tool_git_and_path_rows_have_latest_equal_installed() {
        let installs = parse_crates2(fixture_crates2());
        let git = installs.iter().find(|c| c.name == "my-tool").unwrap();
        let row = to_installed_tool(git, Path::new("/Users/x/.cargo"));
        assert_eq!(row.latest, row.installed.clone().unwrap());

        let path = installs.iter().find(|c| c.name == "local-tool").unwrap();
        let row2 = to_installed_tool(path, Path::new("/Users/x/.cargo"));
        assert_eq!(row2.latest, row2.installed.clone().unwrap());
    }

    #[test]
    fn all_bin_names_covers_every_binary_including_multi_bin_crates() {
        let installs = parse_crates2(fixture_crates2());
        let names = all_bin_names(&installs);
        assert!(names.contains(&"rg".to_string()));
        assert!(names.contains(&"cargo-add".to_string()));
        assert!(names.contains(&"cargo-rm".to_string()));
        assert!(names.contains(&"cargo-set-version".to_string()));
        assert!(names.contains(&"cargo-upgrade".to_string()));
        assert!(names.contains(&"my-tool".to_string()));
        assert!(names.contains(&"local-tool".to_string()));
    }

    #[test]
    fn parses_crates_io_latest_prefers_max_stable() {
        let json = r#"{"crate":{"max_stable_version":"14.1.1","max_version":"15.0.0-rc.1"}}"#;
        assert_eq!(parse_crates_io_latest(json).as_deref(), Some("14.1.1"));
    }

    #[test]
    fn parses_crates_io_latest_falls_back_to_max_version() {
        let json = r#"{"crate":{"max_version":"1.2.3"}}"#;
        assert_eq!(parse_crates_io_latest(json).as_deref(), Some("1.2.3"));
        assert_eq!(parse_crates_io_latest("not json"), None);
        assert_eq!(parse_crates_io_latest(r#"{"nope":true}"#), None);
    }

    #[test]
    fn install_list_fallback_parses_headers_and_binaries() {
        let text = "cargo-audit v0.18.3:\n    cargo-audit\nripgrep v14.1.1:\n    rg\n";
        let installs = parse_install_list(text);
        assert_eq!(installs.len(), 2);
        let audit = installs.iter().find(|c| c.name == "cargo-audit").unwrap();
        assert_eq!(audit.version, "0.18.3");
        assert_eq!(audit.bins, vec!["cargo-audit".to_string()]);
        assert_eq!(audit.source, CrateSource::Registry);
    }

    #[test]
    fn install_list_fallback_classifies_git_and_path_annotations() {
        let text = "my-tool v0.1.0 (https://github.com/user/repo#abc123):\n    my-tool\nlocal-tool v0.2.0 (/Users/me/projects/local-tool):\n    local-tool\n";
        let installs = parse_install_list(text);
        let git = installs.iter().find(|c| c.name == "my-tool").unwrap();
        assert_eq!(git.source, CrateSource::Git);
        let path = installs.iter().find(|c| c.name == "local-tool").unwrap();
        assert_eq!(path.source, CrateSource::Path);
    }

    #[test]
    fn install_list_fallback_empty_or_garbage_yields_no_rows() {
        assert!(parse_install_list("").is_empty());
        assert!(parse_install_list("garbage\nlines\nhere\n").is_empty());
    }

    #[test]
    fn install_root_reads_install_table_root_key() {
        let toml = "[registries]\n[install]\nroot = \"/opt/cargo-custom\"\n";
        assert_eq!(
            parse_install_root(toml).as_deref(),
            Some("/opt/cargo-custom")
        );
    }

    #[test]
    fn install_root_absent_install_table_is_none() {
        let toml = "[registries]\nfoo = \"bar\"\n";
        assert_eq!(parse_install_root(toml), None);
    }

    #[test]
    fn install_root_ignores_root_key_outside_install_table() {
        let toml = "[some-other-table]\nroot = \"/should/not/apply\"\n";
        assert_eq!(parse_install_root(toml), None);
    }

    #[test]
    fn cargo_toml_meta_extracts_description_and_first_author() {
        let toml = "[package]\nname = \"ripgrep\"\ndescription = \"line-oriented search tool\"\nauthors = [\"Andrew Gallant <jamslam@gmail.com>\", \"Other\"]\n";
        let (desc, author) = parse_cargo_toml_meta(toml);
        assert_eq!(desc.as_deref(), Some("line-oriented search tool"));
        assert_eq!(
            author.as_deref(),
            Some("Andrew Gallant <jamslam@gmail.com>")
        );
    }

    #[test]
    fn cargo_toml_meta_missing_fields_are_none() {
        let toml = "[package]\nname = \"x\"\n";
        let (desc, author) = parse_cargo_toml_meta(toml);
        assert_eq!(desc, None);
        assert_eq!(author, None);
    }

    // resolve_cargo_root reads CARGO_INSTALL_ROOT/CARGO_HOME directly (it is
    // the un-memoized inner function; cargo_root() wraps it in a OnceLock for
    // production use, which would make env-var-dependent tests order-flaky).
    // All three scenarios run in ONE test, sequentially, rather than as
    // separate #[test] functions: cargo test runs tests in parallel threads
    // within the same process, and separate tests mutating the same global
    // env vars (CARGO_INSTALL_ROOT/CARGO_HOME) would race each other.
    #[test]
    fn resolve_cargo_root_precedence() {
        let old_root = std::env::var_os("CARGO_INSTALL_ROOT");
        let old_home_env = std::env::var_os("CARGO_HOME");
        let dir = std::env::temp_dir().join(format!(
            "napm-cargo-root-test-{}-{:p}",
            std::process::id(),
            &old_root
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 1. CARGO_INSTALL_ROOT wins over everything, even a CARGO_HOME with
        //    its own install.root config.
        std::fs::write(
            dir.join("config.toml"),
            b"[install]\nroot = \"/opt/cargo-from-config\"\n",
        )
        .unwrap();
        unsafe {
            std::env::set_var("CARGO_INSTALL_ROOT", "/opt/cargo-custom");
            std::env::set_var("CARGO_HOME", &dir);
        }
        assert_eq!(
            resolve_cargo_root(),
            Some(PathBuf::from("/opt/cargo-custom"))
        );

        // 2. No CARGO_INSTALL_ROOT: the CARGO_HOME config's install.root wins.
        unsafe {
            std::env::remove_var("CARGO_INSTALL_ROOT");
        }
        assert_eq!(
            resolve_cargo_root(),
            Some(PathBuf::from("/opt/cargo-from-config"))
        );

        // 3. No config file either: falls back to CARGO_HOME itself.
        std::fs::remove_file(dir.join("config.toml")).unwrap();
        assert_eq!(resolve_cargo_root(), Some(dir.clone()));

        unsafe {
            match old_root {
                Some(v) => std::env::set_var("CARGO_INSTALL_ROOT", v),
                None => std::env::remove_var("CARGO_INSTALL_ROOT"),
            }
            match old_home_env {
                Some(v) => std::env::set_var("CARGO_HOME", v),
                None => std::env::remove_var("CARGO_HOME"),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
