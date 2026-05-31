/// Build the (program, args) for an operation. `pip_bin` is the resolved pip
/// binary (e.g. "pip3"). Returns None for unsupported combinations, notably
/// brew rollback (Homebrew keeps no old bottles).
pub fn build_command(
    eco: &str,
    pkg: &str,
    version: &str,
    action: &str,
    pip_bin: &str,
) -> Option<(String, Vec<String>)> {
    match (eco, action) {
        ("npm", _) => Some((
            "npm".to_string(),
            vec!["i".to_string(), "-g".to_string(), format!("{}@{}", pkg, version)],
        )),
        ("pip", _) => Some((
            pip_bin.to_string(),
            vec!["install".to_string(), format!("{}=={}", pkg, version)],
        )),
        // brew install/update only; no version pinning and no rollback.
        ("brew", "install") | ("brew", "update") => {
            Some(("brew".to_string(), vec!["install".to_string(), pkg.to_string()]))
        }
        // npx Promote to global: install the package globally via npm.
        ("npx", "promote") => Some((
            "npm".to_string(),
            vec!["i".to_string(), "-g".to_string(), pkg.to_string()],
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_install_pins_version() {
        let (prog, args) = build_command("npm", "typescript", "5.6.2", "update", "pip3").unwrap();
        assert_eq!(prog, "npm");
        assert_eq!(args, vec!["i", "-g", "typescript@5.6.2"]);
    }

    #[test]
    fn pip_uses_double_equals_and_given_binary() {
        let (prog, args) = build_command("pip", "httpie", "3.2.2", "rollback", "pip3").unwrap();
        assert_eq!(prog, "pip3");
        assert_eq!(args, vec!["install", "httpie==3.2.2"]);
    }

    #[test]
    fn brew_installs_without_a_version() {
        let (prog, args) = build_command("brew", "ripgrep", "14.1.1", "update", "pip3").unwrap();
        assert_eq!(prog, "brew");
        assert_eq!(args, vec!["install", "ripgrep"]);
    }

    #[test]
    fn npx_promote_installs_globally_via_npm() {
        let (prog, args) = build_command("npx", "create-vite", "5.0.0", "promote", "pip3").unwrap();
        assert_eq!(prog, "npm");
        assert_eq!(args, vec!["i", "-g", "create-vite"]);
    }

    #[test]
    fn brew_rollback_is_unsupported() {
        assert!(build_command("brew", "ripgrep", "14.0.0", "rollback", "pip3").is_none());
    }
}
