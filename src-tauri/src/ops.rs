/// Package-name gate before argv. Rejects: empty, leading '-', any whitespace
/// or control char, and shapes outside the ecosystem's grammar (npm allows one
/// leading @scope/ segment; brew allows '@' for versioned formulae). Length <= 214.
fn valid_pkg(eco: &str, pkg: &str) -> bool {
    if pkg.is_empty() || pkg.len() > 214 {
        return false;
    }
    if pkg.starts_with('-') {
        return false;
    }
    if pkg.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    if pkg.contains("..") {
        return false;
    }
    match eco {
        // npm/npx (npx promote installs via npm): at most one leading
        // @scope/ segment; otherwise no '/' at all.
        "npm" | "npx" => match pkg.strip_prefix('@') {
            Some(rest) => {
                rest.matches('/').count() == 1 && !rest.starts_with('/') && !rest.ends_with('/')
            }
            None => !pkg.contains('/'),
        },
        // pip and brew (and anything else): no path segments. '@' is left
        // unrestricted so brew formulae like "gcc@13" still validate.
        _ => !pkg.contains('/'),
    }
}

/// Version gate: non-empty, no leading '-', chars limited to [A-Za-z0-9._+-].
fn valid_version(v: &str) -> bool {
    if v.is_empty() || v.starts_with('-') {
        return false;
    }
    v.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
}

/// Build the (program, args) for an operation. `pip_bin` is the resolved pip
/// binary (e.g. "pip3"). Returns None for unsupported combinations, notably
/// brew rollback (Homebrew keeps no old bottles), or when `pkg`/`version`
/// fail the argument-injection gate (see valid_pkg/valid_version).
pub fn build_command(
    eco: &str,
    pkg: &str,
    version: &str,
    action: &str,
    pip_bin: &str,
) -> Option<(String, Vec<String>)> {
    if !valid_pkg(eco, pkg) || !valid_version(version) {
        return None;
    }
    match (eco, action) {
        ("npm", _) => Some((
            "npm".to_string(),
            vec![
                "i".to_string(),
                "-g".to_string(),
                "--".to_string(),
                format!("{}@{}", pkg, version),
            ],
        )),
        ("pip", _) => Some((
            pip_bin.to_string(),
            vec![
                "install".to_string(),
                "--".to_string(),
                format!("{}=={}", pkg, version),
            ],
        )),
        // brew install/update only; no version pinning and no rollback.
        // Homebrew does not reliably support a "--" end-of-options marker
        // before formula names, so this arm relies solely on the valid_pkg
        // gate above (leading '-' and '/' are already rejected there).
        ("brew", "install") | ("brew", "update") => Some((
            "brew".to_string(),
            vec!["install".to_string(), pkg.to_string()],
        )),
        // npx Promote to global: install the package globally via npm.
        ("npx", "promote") => Some((
            "npm".to_string(),
            vec![
                "i".to_string(),
                "-g".to_string(),
                "--".to_string(),
                pkg.to_string(),
            ],
        )),
        _ => None,
    }
}

/// Render a (program, args) pair as the shell-ish string shown to the user,
/// e.g. `display_command("pip3", &["install".into(), "httpie==3.2.2".into()])`
/// -> "pip3 install httpie==3.2.2". Display only, not re-parsed or re-executed.
fn display_command(prog: &str, args: &[String]) -> String {
    if args.is_empty() {
        prog.to_string()
    } else {
        format!("{} {}", prog, args.join(" "))
    }
}

use serde::Serialize;
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

/// Read a child pipe to EOF, emitting one event per line. Bytes are decoded
/// lossily (invalid UTF-8 -> U+FFFD) so a stray byte never truncates the
/// stream the way `BufRead::lines()` used to (it returns Err on invalid
/// UTF-8, and the reader threads used to filter-and-stop on that Err via
/// `map_while`, dropping the rest of the output).
/// Splits on \n and also treats \r as a line break so progress-bar rewrites
/// arrive as lines instead of one giant blob. A single \n-delimited read
/// that contains far more \r pieces than a human could ever see distinctly
/// (a progress bar rewriting itself hundreds of times inside one buffered
/// read) is coalesced to just its last piece, so a chatty progress bar
/// cannot flood the UI with hundreds of line events for what is visually
/// one line.
fn stream_lines<R: std::io::Read>(pipe: R, mut emit: impl FnMut(String)) {
    const CR_COALESCE_THRESHOLD: usize = 32;

    let mut reader = BufReader::new(pipe);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        let n = match reader.read_until(b'\n', &mut buf) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }
        if buf.last() == Some(&b'\n') {
            buf.pop();
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
        }
        let text = String::from_utf8_lossy(&buf);
        let pieces: Vec<&str> = text.split('\r').filter(|p| !p.is_empty()).collect();
        if pieces.len() > CR_COALESCE_THRESHOLD {
            if let Some(last) = pieces.last() {
                emit((*last).to_string());
            }
        } else {
            for p in pieces {
                emit(p.to_string());
            }
        }
    }
}

use crate::store::{HistoryEntry, Store};

/// Packages with an operation currently in flight, keyed by (eco, pkg). Not a
/// global queue: different packages still run concurrently. This only rejects
/// a duplicate operation on the SAME package (e.g. a double-click on Get, or
/// Update All somehow enqueuing the same row twice).
static IN_FLIGHT: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

/// Try to claim (eco, pkg) as in flight. Returns false when an operation for
/// it is already running.
fn try_begin(eco: &str, pkg: &str) -> bool {
    let mut guard = IN_FLIGHT.lock().unwrap();
    let set = guard.get_or_insert_with(HashSet::new);
    set.insert((eco.to_string(), pkg.to_string()))
}

/// Release the (eco, pkg) claim taken by `try_begin`.
fn finish(eco: &str, pkg: &str) {
    let mut guard = IN_FLIGHT.lock().unwrap();
    if let Some(set) = guard.as_mut() {
        set.remove(&(eco.to_string(), pkg.to_string()));
    }
}

/// RAII guard that releases the (eco, pkg) in-flight claim on drop, so a
/// panic anywhere in the streaming code cannot leak the claim and permanently
/// block future operations on that package.
struct InFlightGuard {
    eco: String,
    pkg: String,
}
impl Drop for InFlightGuard {
    fn drop(&mut self) {
        finish(&self.eco, &self.pkg);
    }
}

#[derive(Clone, Serialize)]
struct LineEvent {
    op_id: String,
    stream: String, // "stdout" | "stderr"
    line: String,
}

#[derive(Clone, Serialize)]
struct DoneEvent {
    op_id: String,
    success: bool,
    code: i32,
}

/// Spawn the operation on a background thread, streaming `transfer-line` events
/// and a final `transfer-done`. On success, log a HistoryEntry to `store`.
/// Returns immediately. `ts` is the current Unix time (the caller stamps it).
#[allow(clippy::too_many_arguments)]
pub fn run_op(
    app: AppHandle,
    store: Store,
    op_id: String,
    eco: String,
    pkg: String,
    from: Option<String>,
    to: String,
    action: String,
    ts: i64,
) {
    if !try_begin(&eco, &pkg) {
        let _ = app.emit(
            "transfer-line",
            LineEvent {
                op_id: op_id.clone(),
                stream: "stderr".into(),
                line: format!("another operation for {} is already running", pkg),
            },
        );
        let _ = app.emit(
            "transfer-done",
            DoneEvent {
                op_id,
                success: false,
                code: -1,
            },
        );
        return;
    }

    let pip = crate::scan::pip::pip_bin().unwrap_or("pip3");
    let built = build_command(&eco, &pkg, &to, &action, pip);

    std::thread::spawn(move || {
        let _guard = InFlightGuard {
            eco: eco.clone(),
            pkg: pkg.clone(),
        };
        let (prog, args) = match built {
            Some(c) => c,
            None => {
                let _ = app.emit(
                    "transfer-line",
                    LineEvent {
                        op_id: op_id.clone(),
                        stream: "stderr".into(),
                        line: format!("unsupported operation: {} {}", eco, action),
                    },
                );
                let _ = app.emit(
                    "transfer-done",
                    DoneEvent {
                        op_id: op_id.clone(),
                        success: false,
                        code: -1,
                    },
                );
                return;
            }
        };

        let mut child = match Command::new(&prog)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = app.emit(
                    "transfer-line",
                    LineEvent {
                        op_id: op_id.clone(),
                        stream: "stderr".into(),
                        line: format!("failed to start {}: {}", prog, e),
                    },
                );
                let _ = app.emit(
                    "transfer-done",
                    DoneEvent {
                        op_id: op_id.clone(),
                        success: false,
                        code: -1,
                    },
                );
                return;
            }
        };

        // The command actually executed, as the first line of output, so the
        // Transfers row shows the truth rather than a string reconstructed in
        // JS (which never knew, e.g., that pip resolved to "pip3").
        let _ = app.emit(
            "transfer-line",
            LineEvent {
                op_id: op_id.clone(),
                stream: "stdout".into(),
                line: format!("$ {}", display_command(&prog, &args)),
            },
        );

        let mut handles = Vec::new();
        if let Some(pipe) = child.stdout.take() {
            let app2 = app.clone();
            let id2 = op_id.clone();
            handles.push(std::thread::spawn(move || {
                stream_lines(pipe, |line| {
                    let _ = app2.emit(
                        "transfer-line",
                        LineEvent {
                            op_id: id2.clone(),
                            stream: "stdout".into(),
                            line,
                        },
                    );
                });
            }));
        }
        if let Some(pipe) = child.stderr.take() {
            let app2 = app.clone();
            let id2 = op_id.clone();
            handles.push(std::thread::spawn(move || {
                stream_lines(pipe, |line| {
                    let _ = app2.emit(
                        "transfer-line",
                        LineEvent {
                            op_id: id2.clone(),
                            stream: "stderr".into(),
                            line,
                        },
                    );
                });
            }));
        }

        let status = child.wait();
        for h in handles {
            let _ = h.join();
        }

        let code = status
            .as_ref()
            .map(|s| s.code().unwrap_or(-1))
            .unwrap_or(-1);
        let success = status.map(|s| s.success()).unwrap_or(false);

        if success {
            store.add_history(HistoryEntry {
                ts,
                pkg,
                eco,
                action,
                from,
                to,
            });
        }
        let _ = app.emit(
            "transfer-done",
            DoneEvent {
                op_id,
                success,
                code,
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_install_pins_version() {
        let (prog, args) = build_command("npm", "typescript", "5.6.2", "update", "pip3").unwrap();
        assert_eq!(prog, "npm");
        assert_eq!(args, vec!["i", "-g", "--", "typescript@5.6.2"]);
    }

    #[test]
    fn npm_install_supports_scoped_package_name() {
        let (prog, args) = build_command("npm", "@vue/cli", "5.0.8", "update", "pip3").unwrap();
        assert_eq!(prog, "npm");
        assert_eq!(args, vec!["i", "-g", "--", "@vue/cli@5.0.8"]);
    }

    #[test]
    fn pip_uses_double_equals_and_given_binary() {
        let (prog, args) = build_command("pip", "httpie", "3.2.2", "rollback", "pip3").unwrap();
        assert_eq!(prog, "pip3");
        assert_eq!(args, vec!["install", "--", "httpie==3.2.2"]);
    }

    #[test]
    fn brew_installs_without_a_version() {
        let (prog, args) = build_command("brew", "ripgrep", "14.1.1", "update", "pip3").unwrap();
        assert_eq!(prog, "brew");
        assert_eq!(args, vec!["install", "ripgrep"]);
    }

    #[test]
    fn brew_formula_name_may_contain_at_version_suffix() {
        let (prog, args) = build_command("brew", "gcc@13", "13.2.0", "update", "pip3").unwrap();
        assert_eq!(prog, "brew");
        assert_eq!(args, vec!["install", "gcc@13"]);
    }

    #[test]
    fn npx_promote_installs_globally_via_npm() {
        let (prog, args) = build_command("npx", "create-vite", "5.0.0", "promote", "pip3").unwrap();
        assert_eq!(prog, "npm");
        assert_eq!(args, vec!["i", "-g", "--", "create-vite"]);
    }

    #[test]
    fn brew_rollback_is_unsupported() {
        assert!(build_command("brew", "ripgrep", "14.0.0", "rollback", "pip3").is_none());
    }

    #[test]
    fn rejects_leading_dash_package_name() {
        assert!(build_command("npm", "-rf", "1.0.0", "update", "pip3").is_none());
    }

    #[test]
    fn rejects_package_name_with_a_space() {
        assert!(build_command("npm", "evil pkg", "1.0.0", "update", "pip3").is_none());
    }

    #[test]
    fn rejects_package_name_with_a_newline() {
        assert!(build_command("npm", "evil\npkg", "1.0.0", "update", "pip3").is_none());
    }

    #[test]
    fn rejects_dotdot_in_package_name() {
        assert!(build_command("npm", "..", "1.0.0", "update", "pip3").is_none());
    }

    #[test]
    fn rejects_multi_segment_scoped_name() {
        assert!(build_command("npm", "@scope/sub/pkg", "1.0.0", "update", "pip3").is_none());
    }

    #[test]
    fn rejects_slash_in_pip_or_brew_package_name() {
        assert!(build_command("pip", "evil/pkg", "1.0.0", "update", "pip3").is_none());
        assert!(build_command("brew", "evil/pkg", "1.0.0", "update", "pip3").is_none());
    }

    #[test]
    fn rejects_empty_version() {
        assert!(build_command("npm", "typescript", "", "update", "pip3").is_none());
    }

    #[test]
    fn rejects_version_with_a_space() {
        assert!(build_command("npm", "typescript", "1.0 0", "update", "pip3").is_none());
    }

    #[test]
    fn rejects_leading_dash_version() {
        assert!(build_command("npm", "typescript", "-1.0.0", "update", "pip3").is_none());
    }

    #[test]
    fn display_command_uses_the_resolved_pip_binary() {
        let args = vec!["install".to_string(), "httpie==3.2.2".to_string()];
        assert_eq!(display_command("pip3", &args), "pip3 install httpie==3.2.2");
    }

    #[test]
    fn display_command_with_no_args_is_just_the_program() {
        assert_eq!(display_command("brew", &[]), "brew");
    }

    // IN_FLIGHT is process-global state shared across these tests, and tests
    // may run in parallel, so each test below uses a package name unique to
    // it (not shared with any other test in this module) to avoid cross-test
    // interference.

    #[test]
    fn try_begin_rejects_duplicate_then_allows_after_finish() {
        assert!(try_begin("npm", "in-flight-dup-test"));
        assert!(!try_begin("npm", "in-flight-dup-test"));
        finish("npm", "in-flight-dup-test");
        assert!(try_begin("npm", "in-flight-dup-test"));
        finish("npm", "in-flight-dup-test");
    }

    #[test]
    fn try_begin_is_independent_per_ecosystem() {
        assert!(try_begin("npm", "in-flight-eco-test"));
        assert!(try_begin("pip", "in-flight-eco-test"));
        finish("npm", "in-flight-eco-test");
        finish("pip", "in-flight-eco-test");
    }

    #[test]
    fn stream_lines_round_trips_plain_lines() {
        let input = b"one\ntwo\nthree\n".to_vec();
        let mut out = Vec::new();
        stream_lines(std::io::Cursor::new(input), |l| out.push(l));
        assert_eq!(out, vec!["one", "two", "three"]);
    }

    #[test]
    fn stream_lines_lossy_decodes_invalid_utf8_and_keeps_going() {
        // A stray invalid-UTF-8 byte used to end the whole iterator via
        // BufRead::lines()'s Err propagating through the old filter-and-stop
        // adapter. It must now decode to U+FFFD and let later lines through.
        let input = b"ok\n\xFF\nafter\n".to_vec();
        let mut out = Vec::new();
        stream_lines(std::io::Cursor::new(input), |l| out.push(l));
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], "ok");
        assert!(out[1].contains('\u{FFFD}'));
        assert_eq!(out[2], "after");
    }

    #[test]
    fn stream_lines_splits_on_cr_for_progress_output() {
        let input = b"10%\r50%\r100%\n".to_vec();
        let mut out = Vec::new();
        stream_lines(std::io::Cursor::new(input), |l| out.push(l));
        assert_eq!(out, vec!["10%", "50%", "100%"]);
    }

    #[test]
    fn stream_lines_emits_final_partial_line_without_trailing_newline() {
        let input = b"no newline at end".to_vec();
        let mut out = Vec::new();
        stream_lines(std::io::Cursor::new(input), |l| out.push(l));
        assert_eq!(out, vec!["no newline at end"]);
    }

    #[test]
    fn stream_lines_coalesces_a_flood_of_cr_pieces_to_the_last() {
        let mut input = String::new();
        for i in 0..200 {
            input.push_str(&i.to_string());
            input.push('\r');
        }
        input.push_str("done\n");
        let mut out = Vec::new();
        stream_lines(std::io::Cursor::new(input.into_bytes()), |l| out.push(l));
        assert_eq!(out, vec!["done"]);
    }
}
