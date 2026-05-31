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

use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter};

use crate::store::{HistoryEntry, Store};

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
    let pip = crate::scan::pip::pip_bin().unwrap_or("pip3");
    let built = build_command(&eco, &pkg, &to, &action, pip);

    std::thread::spawn(move || {
        let (prog, args) = match built {
            Some(c) => c,
            None => {
                let _ = app.emit(
                    "transfer-done",
                    DoneEvent { op_id: op_id.clone(), success: false, code: -1 },
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
                    LineEvent { op_id: op_id.clone(), stream: "stderr".into(), line: format!("failed to start {}: {}", prog, e) },
                );
                let _ = app.emit("transfer-done", DoneEvent { op_id: op_id.clone(), success: false, code: -1 });
                return;
            }
        };

        let mut handles = Vec::new();
        if let Some(pipe) = child.stdout.take() {
            let app2 = app.clone();
            let id2 = op_id.clone();
            handles.push(std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    let _ = app2.emit("transfer-line", LineEvent { op_id: id2.clone(), stream: "stdout".into(), line });
                }
            }));
        }
        if let Some(pipe) = child.stderr.take() {
            let app2 = app.clone();
            let id2 = op_id.clone();
            handles.push(std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    let _ = app2.emit("transfer-line", LineEvent { op_id: id2.clone(), stream: "stderr".into(), line });
                }
            }));
        }

        let status = child.wait();
        for h in handles {
            let _ = h.join();
        }

        let code = status.as_ref().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        let success = status.map(|s| s.success()).unwrap_or(false);

        if success {
            store.add_history(HistoryEntry { ts, pkg, eco, action, from, to });
        }
        let _ = app.emit("transfer-done", DoneEvent { op_id, success, code });
    });
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
