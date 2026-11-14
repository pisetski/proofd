use super::cli_provider::{CliRun, cli_sandbox_dir, run_cli};
use super::cli_resolver::resolve_cli;
use super::sanitize::sanitize_output;
use super::{Provider, ProviderError};
use serde::Deserialize;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub const FORGET_SESSIONS_FLAG: &str = "--forget-opencode-sessions";
const AGENT: &str = "proofd";

#[derive(Debug, Clone)]
pub struct OpencodeCli {
    pub model: String,
    pub bin: Option<String>,
}

impl Provider for OpencodeCli {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        let bin = resolve_cli(self.bin.as_deref(), "opencode")?;
        let sandbox = cli_sandbox_dir();
        let _forget = sandbox.as_ref().map(|dir| ForgetSessionsOnDrop {
            bin: bin.clone(),
            dir: dir.clone(),
        });
        let config = opencode_config(system);
        let env = opencode_env(&config);
        let stdout = run_cli(CliRun {
            name: "opencode",
            explicit_bin: Some(&bin),
            args: &opencode_args(&self.model, sandbox.as_deref()),
            env: &env,
            stdin_text: user,
            cwd: sandbox.as_deref(),
            fatal_stderr_line: stream_error,
        })
        .await?;
        sanitize_output(&reply_text(&stdout))
    }
}

// Error-level logs are the only sign of a rejected request (e.g. a spent free quota):
// opencode keeps retrying it silently until proofd times out.
pub fn opencode_args(model: &str, sandbox: Option<&Path>) -> Vec<String> {
    let mut args = [
        "run",
        "--agent",
        AGENT,
        "--model",
        model,
        "--format",
        "json",
        "--print-logs",
        "--log-level",
        "ERROR",
    ]
    .map(String::from)
    .to_vec();
    if let Some(dir) = sandbox {
        args.extend(["--dir".to_string(), dir.display().to_string()]);
    }
    args
}

// Selected text is untrusted: a selection saying "run this command" must have no tool to run it with.
pub fn opencode_config(system: &str) -> String {
    serde_json::json!({
        "agent": {
            AGENT: {
                "mode": "primary",
                "prompt": system,
                "permission": { "*": "deny" },
            },
            "title": { "disable": true },
        },
        "share": "disabled",
        "autoupdate": false,
    })
    .to_string()
}

fn opencode_env(config: &str) -> [(&str, &str); 6] {
    [
        ("OPENCODE_CONFIG_CONTENT", config),
        ("OPENCODE_DISABLE_PROJECT_CONFIG", "1"),
        ("OPENCODE_DISABLE_CLAUDE_CODE", "1"),
        ("OPENCODE_DISABLE_EXTERNAL_SKILLS", "1"),
        ("OPENCODE_DISABLE_SHARE", "1"),
        ("OPENCODE_DISABLE_AUTOUPDATE", "1"),
    ]
}

pub fn stream_error(log_line: &str) -> Option<String> {
    if !log_line.contains(r#"message="stream error""#) {
        return None;
    }
    let reason = match log_line.split_once(r#"error.error=""#) {
        Some((_, quoted)) => quoted.trim_end().trim_end_matches('"'),
        None => log_line.trim(),
    };
    Some(reason.to_string())
}

pub fn reply_text(json_events: &str) -> String {
    json_events
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event["type"] == "text")
        .filter_map(|event| event["part"]["text"].as_str().map(String::from))
        .collect()
}

// Runs on timeout too, when the future is dropped mid-flight.
struct ForgetSessionsOnDrop {
    bin: String,
    dir: PathBuf,
}

impl Drop for ForgetSessionsOnDrop {
    fn drop(&mut self) {
        let Ok(proofd) = std::env::current_exe() else {
            tracing::warn!("cannot locate proofd to delete opencode sessions");
            return;
        };
        let spawned = Command::new(proofd)
            .arg(FORGET_SESSIONS_FLAG)
            .arg(&self.bin)
            .arg(&self.dir)
            .arg(now_ms().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn();
        if let Err(e) = spawned {
            tracing::warn!("cannot delete opencode sessions: {e}");
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct Session {
    id: String,
    directory: PathBuf,
    created: u128,
}

// A session created after the cutoff belongs to a proofread still in flight.
pub fn sessions_to_forget(list_json: &str, dir: &Path, cutoff_ms: u128) -> Vec<String> {
    serde_json::from_str::<Vec<Session>>(list_json)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| same_dir(&s.directory, dir) && s.created <= cutoff_ms)
        .map(|s| s.id)
        .collect()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

pub fn forget_sessions(bin: &str, dir: &Path, cutoff_ms: u128) -> Result<(), String> {
    let list = Command::new(bin)
        .args(["session", "list", "--format", "json"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("list opencode sessions: {e}"))?;
    if !list.status.success() {
        return Err(format!(
            "list opencode sessions: {}",
            String::from_utf8_lossy(&list.stderr).trim()
        ));
    }
    for id in sessions_to_forget(&String::from_utf8_lossy(&list.stdout), dir, cutoff_ms) {
        Command::new(bin)
            .args(["session", "delete", &id])
            .current_dir(dir)
            .output()
            .map_err(|e| format!("delete opencode session {id}: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_select_the_locked_down_agent_and_json_output() {
        let args = opencode_args("opencode/m", Some(Path::new("/sandbox")));
        let has_pair = |flag: &str, value: &str| {
            args.windows(2)
                .any(|pair| pair[0] == flag && pair[1] == value)
        };
        assert_eq!(args[0], "run");
        assert!(has_pair("--agent", AGENT));
        assert!(has_pair("--model", "opencode/m"));
        assert!(has_pair("--format", "json"));
        assert!(has_pair("--dir", "/sandbox"));
    }

    #[test]
    fn config_denies_every_tool_and_skips_title_generation() {
        let config: serde_json::Value = serde_json::from_str(&opencode_config("SYSTEM")).unwrap();
        let agent = &config["agent"][AGENT];
        assert_eq!(agent["prompt"], "SYSTEM");
        assert_eq!(agent["permission"]["*"], "deny");
        assert_eq!(config["agent"]["title"]["disable"], true);
        assert_eq!(config["share"], "disabled");
    }

    #[test]
    fn stream_error_takes_the_reason_from_opencodes_error_log() {
        let line = r#"timestamp=2026-11-14T15:17:58.057Z level=ERROR run=bf2446a5 message="stream error" providerID=opencode agent=proofd error.error="AI_APICallError: Free usage limit reached. Subscribe: https://opencode.ai/go"
"#;
        assert_eq!(
            stream_error(line).as_deref(),
            Some("AI_APICallError: Free usage limit reached. Subscribe: https://opencode.ai/go")
        );
        assert_eq!(
            stream_error(r#"level=ERROR message="stream error" code=429"#).as_deref(),
            Some(r#"level=ERROR message="stream error" code=429"#)
        );
        assert_eq!(stream_error(r#"level=ERROR message="plugin failed""#), None);
    }

    #[test]
    fn reply_text_joins_text_parts_and_ignores_other_events() {
        let events = [
            r#"{"type":"step_start","sessionID":"ses_1","part":{"type":"step-start"}}"#,
            r#"{"type":"text","sessionID":"ses_1","part":{"type":"text","text":"Hello, "}}"#,
            "not json",
            r#"{"type":"text","sessionID":"ses_1","part":{"type":"text","text":"world."}}"#,
            r#"{"type":"step_finish","sessionID":"ses_1","part":{"type":"step-finish"}}"#,
        ]
        .join("\n");
        assert_eq!(reply_text(&events), "Hello, world.");
    }

    #[test]
    fn forgets_only_finished_sessions_in_the_sandbox() {
        let list = r#"[
            {"id":"ses_old","directory":"/sandbox","created":100,"updated":100,"title":"t"},
            {"id":"ses_in_flight","directory":"/sandbox","created":300,"updated":300,"title":"t"},
            {"id":"ses_user","directory":"/Users/me/project","created":100,"updated":100,"title":"t"}
        ]"#;
        assert_eq!(
            sessions_to_forget(list, Path::new("/sandbox"), 200),
            vec!["ses_old".to_string()]
        );
    }

    #[test]
    fn unparseable_session_list_forgets_nothing() {
        assert!(sessions_to_forget("oops", Path::new("/sandbox"), 200).is_empty());
    }
}
