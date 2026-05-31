use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const START: &str = "__NAPM_PATH_START__";
const END: &str = "__NAPM_PATH_END__";

/// Pull the PATH from sentinel-delimited shell output. Returns the trimmed
/// substring between the markers, or None if a marker is absent or the content
/// is empty. Robust to rc-file noise printed around the markers.
pub fn extract_path(output: &str) -> Option<String> {
    let start = output.find(START)? + START.len();
    let rest = &output[start..];
    let end = rest.find(END)? + start;
    let p = output[start..end].trim();
    if p.is_empty() {
        None
    } else {
        Some(p.to_string())
    }
}

/// Capture the user's real login-shell PATH and set it on this process so every
/// child spawned afterward inherits it. A no-op when the shell probe fails (the
/// inherited PATH is left untouched). Call once at the very start of `run()`,
/// before any process is spawned, so Finder/Dock launches can find npm/brew/pip
/// and the manual scanner can walk a real $PATH.
pub fn fix_path() {
    if let Some(p) = capture_login_path() {
        std::env::set_var("PATH", p);
    }
}

/// Run the user's login+interactive shell with a sentinel-delimited probe and
/// extract the PATH it reports. None on any failure. The result must contain a
/// '/' as a minimal sanity check before it is trusted.
fn capture_login_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let probe = "printf '__NAPM_PATH_START__%s__NAPM_PATH_END__' \"$PATH\"";
    let out = run_shell(&shell, &["-ilc", probe], Duration::from_millis(2000))?;
    let p = extract_path(&out)?;
    if p.contains('/') {
        Some(p)
    } else {
        None
    }
}

/// Spawn `shell args...`, capture stdout, and kill it if it exceeds `dur`.
/// stderr is discarded so rc-file warnings never pollute the result.
fn run_shell(shell: &str, args: &[&str], dur: Duration) -> Option<String> {
    let mut child = Command::new(shell)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() >= dur {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_clean_path() {
        let out = "__NAPM_PATH_START__/opt/homebrew/bin:/usr/bin__NAPM_PATH_END__";
        assert_eq!(extract_path(out).as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
    }

    #[test]
    fn extracts_through_rc_noise() {
        let out = "welcome to your shell\nsome banner\n__NAPM_PATH_START__/usr/local/bin:/usr/bin__NAPM_PATH_END__\n";
        assert_eq!(extract_path(out).as_deref(), Some("/usr/local/bin:/usr/bin"));
    }

    #[test]
    fn missing_markers_is_none() {
        assert_eq!(extract_path("no markers here /usr/bin"), None);
        assert_eq!(extract_path("__NAPM_PATH_START__/usr/bin no end"), None);
    }

    #[test]
    fn empty_between_markers_is_none() {
        assert_eq!(extract_path("__NAPM_PATH_START____NAPM_PATH_END__"), None);
        assert_eq!(extract_path("__NAPM_PATH_START__   __NAPM_PATH_END__"), None);
    }
}
