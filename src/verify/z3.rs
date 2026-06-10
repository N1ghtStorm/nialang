//! Z3 subprocess manager (phase 11).

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtResult {
    Unsat,
    Sat,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Z3Config {
    pub z3_path: String,
    pub timeout_secs: u32,
}

impl Default for Z3Config {
    fn default() -> Self {
        Self {
            z3_path: "z3".into(),
            timeout_secs: 5,
        }
    }
}

static CACHE: Mutex<Option<HashMap<String, SmtResult>>> = Mutex::new(None);

fn cache() -> std::sync::MutexGuard<'static, Option<HashMap<String, SmtResult>>> {
    let mut guard = CACHE.lock().expect("z3 cache lock");
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
}

/// Returns the path to `z3` if it is available on PATH.
pub fn find_z3() -> Option<String> {
    let path = std::env::var_os("Z3_PATH")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "z3".into());
    let ok = Command::new(&path)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        Some(path)
    } else {
        None
    }
}

/// Runs Z3 on an SMT-LIB script; `Unsat` means the goal is discharged.
pub fn solve(script: &str, config: &Z3Config) -> Result<SmtResult, String> {
    if let Some(cached) = cache().as_ref().and_then(|c| c.get(script).copied()) {
        return Ok(cached);
    }

    let mut child = Command::new(&config.z3_path)
        .args([
            &format!("-T:{}", config.timeout_secs),
            "-in",
            "-smt2",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn `{}`: {e}", config.z3_path))?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| "failed to open z3 stdin".to_string())?
        .write_all(script.as_bytes())
        .map_err(|e| format!("failed to write SMT script to z3: {e}"))?;

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for z3: {e}"))?;

    if !output.status.success() && output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "z3 exited with status {:?}: {stderr}",
            output.status.code()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result = parse_z3_output(&stdout)?;
    cache()
        .as_mut()
        .expect("cache initialized")
        .insert(script.to_string(), result);
    Ok(result)
}

fn parse_z3_output(stdout: &str) -> Result<SmtResult, String> {
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match line {
            "unsat" => return Ok(SmtResult::Unsat),
            "sat" => return Ok(SmtResult::Sat),
            "unknown" => return Ok(SmtResult::Unknown),
            _ if line.starts_with("(") => continue,
            _ => continue,
        }
    }
    Err(format!("unexpected z3 output: {stdout:?}"))
}

/// Clears the solver result cache (for tests).
pub fn clear_cache() {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(HashMap::new());
    }
}

#[allow(dead_code)]
pub fn default_timeout() -> Duration {
    Duration::from_secs(Z3Config::default().timeout_secs as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn z3_proves_trivial_contradiction() {
        let Some(path) = find_z3() else {
            eprintln!("skipping z3 test: z3 not found in PATH");
            return;
        };
        clear_cache();
        let script = "\
(set-logic QF_LIA)
(declare-fun x () Int)
(assert (= x 2))
(assert (not (distinct x 0)))
(check-sat)
";
        let result = solve(
            script,
            &Z3Config {
                z3_path: path,
                timeout_secs: 5,
            },
        )
        .expect("solve");
        assert_eq!(result, SmtResult::Unsat);
    }
}
