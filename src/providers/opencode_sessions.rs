use super::cli_resolver::path_for_child;
use serde::Deserialize;
use std::fs::{File, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub const FORGET_SESSIONS_FLAG: &str = "--forget-opencode-sessions";
const SERVICE_LOG_IN_HOME: &str = "Library/Logs/proofd-service.log";

pub(super) struct ForgetSessionsOnDrop {
    opencode: String,
    sandbox: PathBuf,
}

impl ForgetSessionsOnDrop {
    pub(super) fn new(opencode: &str, sandbox: &Path) -> Self {
        Self {
            opencode: opencode.to_string(),
            sandbox: sandbox.to_path_buf(),
        }
    }
}

impl Drop for ForgetSessionsOnDrop {
    fn drop(&mut self) {
        if let Err(e) = spawn_detached_forget(&self.opencode, &self.sandbox, now_ms()) {
            tracing::warn!("cannot delete opencode sessions: {e}");
        }
    }
}

fn spawn_detached_forget(opencode: &str, sandbox: &Path, cutoff_ms: u128) -> std::io::Result<()> {
    Command::new(std::env::current_exe()?)
        .arg(FORGET_SESSIONS_FLAG)
        .arg(opencode)
        .arg(sandbox)
        .arg(cutoff_ms.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(service_log_to_append_to().map_or_else(Stdio::null, Stdio::from))
        .process_group(0)
        .spawn()
        .map(drop)
}

fn service_log_to_append_to() -> Option<File> {
    let path = dirs::home_dir()?.join(SERVICE_LOG_IN_HOME);
    std::fs::create_dir_all(path.parent()?).ok()?;
    OpenOptions::new().create(true).append(true).open(path).ok()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

pub fn forget_sessions(opencode: &str, sandbox: &Path, cutoff_ms: u128) -> Result<(), String> {
    let list = Command::new(opencode)
        .args(["session", "list", "--format", "json"])
        .env("PATH", path_for_child(opencode))
        .current_dir(sandbox)
        .output()
        .map_err(|e| format!("list opencode sessions: {e}"))?;
    if !list.status.success() {
        return Err(format!(
            "list opencode sessions: {}",
            String::from_utf8_lossy(&list.stderr).trim()
        ));
    }
    let session_list = String::from_utf8_lossy(&list.stdout);
    let failures: Vec<String> =
        sandbox_sessions_created_at_or_before(&session_list, sandbox, cutoff_ms)?
            .iter()
            .filter_map(|id| delete_session(opencode, sandbox, id).err())
            .collect();
    if failures.is_empty() {
        return Ok(());
    }
    Err(failures.join("; "))
}

fn delete_session(opencode: &str, sandbox: &Path, id: &str) -> Result<(), String> {
    let deleted = Command::new(opencode)
        .args(["session", "delete", id])
        .env("PATH", path_for_child(opencode))
        .current_dir(sandbox)
        .output()
        .map_err(|e| format!("delete opencode session {id}: {e}"))?;
    if deleted.status.success() {
        return Ok(());
    }
    Err(format!(
        "delete opencode session {id} ({}): {}",
        deleted.status,
        String::from_utf8_lossy(&deleted.stderr).trim()
    ))
}

#[derive(Debug, Deserialize)]
struct Session {
    id: String,
    directory: PathBuf,
    created: u128,
}

fn sandbox_sessions_created_at_or_before(
    list_json: &str,
    sandbox: &Path,
    cutoff_ms: u128,
) -> Result<Vec<String>, String> {
    let sessions = serde_json::from_str::<Vec<Session>>(list_json)
        .map_err(|_| "list opencode sessions: output not recognized (did its format change?)")?;
    Ok(sessions
        .into_iter()
        .filter(|session| same_dir(&session.directory, sandbox) && session.created <= cutoff_ms)
        .map(|session| session.id)
        .collect())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgets_only_sandbox_sessions_that_existed_at_the_cutoff() {
        let list = r#"[
            {"id":"ses_old","directory":"/sandbox","created":100,"updated":100,"title":"t"},
            {"id":"ses_in_flight","directory":"/sandbox","created":300,"updated":300,"title":"t"},
            {"id":"ses_user","directory":"/Users/me/project","created":100,"updated":100,"title":"t"}
        ]"#;
        assert_eq!(
            sandbox_sessions_created_at_or_before(list, Path::new("/sandbox"), 200),
            Ok(vec!["ses_old".to_string()])
        );
    }
}
