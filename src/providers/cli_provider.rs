use super::cli_resolver::resolve_cli;
use super::sanitize::sanitize_output;
use super::{Provider, ProviderError};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::OnceLock;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, Command};

#[derive(Debug, Clone)]
pub struct ClaudeCli {
    pub model: String,
    pub bin: Option<String>,
}

impl Provider for ClaudeCli {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        let stdout = run_cli(CliRun {
            name: "claude",
            explicit_bin: self.bin.as_deref(),
            args: &claude_args(&self.model, system),
            env: &CLAUDE_ENV,
            stdin_text: user,
            cwd: cli_sandbox_dir().as_deref(),
            fatal_stderr_line: no_fatal_stderr_line,
        })
        .await?;
        sanitize_output(&stdout)
    }
}

// Telemetry, update checks, and feature-flag fetches added ~1.5s to every claude startup.
const CLAUDE_ENV: [(&str, &str); 1] = [("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")];

fn claude_args(model: &str, system: &str) -> Vec<String> {
    [
        "-p",
        "--model",
        model,
        "--output-format",
        "text",
        "--tools",
        "",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--disable-slash-commands",
        "--setting-sources",
        "",
        "--system-prompt",
        system,
    ]
    .map(String::from)
    .to_vec()
}

pub(super) struct CliRun<'a> {
    pub name: &'a str,
    pub explicit_bin: Option<&'a str>,
    pub args: &'a [String],
    pub env: &'a [(&'a str, &'a str)],
    pub stdin_text: &'a str,
    pub cwd: Option<&'a Path>,
    pub fatal_stderr_line: fn(&str) -> Option<String>,
}

fn no_fatal_stderr_line(_line: &str) -> Option<String> {
    None
}

pub(super) async fn run_cli(run: CliRun<'_>) -> Result<String, ProviderError> {
    let name = run.name;
    let transport =
        |what: &str, e: std::io::Error| ProviderError::Transport(format!("{what}: {e}"));
    let total = Instant::now();
    let bin = resolve_cli(run.explicit_bin, name)?;
    let resolve_ms = total.elapsed().as_millis();
    let mut command = Command::new(&bin);
    command
        .args(run.args)
        .envs(run.env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = run.cwd {
        command.current_dir(dir);
    }
    let mut child = command
        .spawn()
        .map_err(|e| transport(&format!("spawn {name} ({bin})"), e))?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    let stdin_text = run.stdin_text;
    let wait = Instant::now();
    let (_, stdout, stderr, status) = tokio::try_join!(
        async move {
            let written = stdin.write_all(stdin_text.as_bytes()).await;
            drop(stdin);
            match written {
                Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => {
                    Err(transport(&format!("write {name} stdin"), e))
                }
                _ => Ok(()),
            }
        },
        async {
            let mut buf = Vec::new();
            stdout
                .read_to_end(&mut buf)
                .await
                .map_err(|e| transport(&format!("read {name} stdout"), e))?;
            Ok(buf)
        },
        watch_stderr(name, stderr, run.fatal_stderr_line),
        async {
            child
                .wait()
                .await
                .map_err(|e| transport(&format!("wait {name}"), e))
        },
    )?;
    tracing::debug!(
        "{name} timings resolve={resolve_ms}ms wait={}ms total={}ms",
        wait.elapsed().as_millis(),
        total.elapsed().as_millis(),
    );
    if !status.success() {
        return Err(ProviderError::Failed(exit_failure(name, status, &stderr)));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

// Returning early drops the child, which kill_on_drop then stops.
async fn watch_stderr(
    name: &str,
    stderr: ChildStderr,
    fatal_stderr_line: fn(&str) -> Option<String>,
) -> Result<String, ProviderError> {
    let mut reader = BufReader::new(stderr);
    let mut seen = String::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .await
            .map_err(|e| ProviderError::Transport(format!("read {name} stderr: {e}")))?;
        if read == 0 {
            return Ok(seen);
        }
        let text = String::from_utf8_lossy(&line);
        if let Some(reason) = fatal_stderr_line(&text) {
            return Err(ProviderError::Failed(format!("{name}: {reason}")));
        }
        seen.push_str(&text);
    }
}

fn exit_failure(name: &str, status: ExitStatus, stderr: &str) -> String {
    let how = match status.code() {
        Some(code) => format!("exited with code {code}"),
        None => format!(
            "was killed by signal {}",
            status.signal().unwrap_or_default()
        ),
    };
    match stderr.trim() {
        "" => format!("{name} {how}"),
        detail => format!("{name} {how}: {detail}"),
    }
}

// Agentic CLIs pick up skills and project files from their cwd, which turned revisions into essays.
pub fn cli_sandbox_dir() -> Option<PathBuf> {
    static SANDBOX: OnceLock<Option<PathBuf>> = OnceLock::new();
    SANDBOX
        .get_or_init(|| {
            let base = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
            let dir = base.join("proofd").join("cli-sandbox");
            match std::fs::create_dir_all(&dir) {
                Ok(()) => Some(dir),
                Err(e) => {
                    tracing::warn!("cli sandbox unavailable at {}: {e}", dir.display());
                    None
                }
            }
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_sandbox_is_stable_and_empty_of_skills() {
        let a = cli_sandbox_dir().expect("sandbox must exist in test env");
        let b = cli_sandbox_dir().expect("sandbox must be cached");
        assert_eq!(a, b);
        assert!(a.is_dir(), "got: {}", a.display());
        for entry in ["AGENTS.md", "SKILL.md", "skills", ".opencode"] {
            assert!(
                !a.join(entry).exists(),
                "sandbox contaminated: {}",
                a.join(entry).display()
            );
        }
    }

    #[test]
    fn claude_args_isolate_the_agent_and_pass_system_prompt_as_flag() {
        let args = claude_args("haiku", "SYSTEM");
        let has_pair = |flag: &str, value: &str| {
            args.windows(2)
                .any(|pair| pair[0] == flag && pair[1] == value)
        };
        assert!(has_pair("--tools", ""));
        assert!(has_pair("--setting-sources", ""));
        assert!(has_pair("--system-prompt", "SYSTEM"));
        assert!(has_pair("--model", "haiku"));
        for flag in [
            "--strict-mcp-config",
            "--no-session-persistence",
            "--disable-slash-commands",
        ] {
            assert!(args.contains(&flag.to_string()), "missing {flag}");
        }
    }

    #[test]
    fn exit_failure_names_the_code_and_omits_empty_stderr() {
        let exit_1 = ExitStatus::from_raw(1 << 8);
        assert_eq!(
            exit_failure("claude", exit_1, " \n"),
            "claude exited with code 1"
        );
        assert_eq!(
            exit_failure("claude", exit_1, "key expired\n"),
            "claude exited with code 1: key expired"
        );
        let killed = ExitStatus::from_raw(9);
        assert_eq!(
            exit_failure("opencode", killed, ""),
            "opencode was killed by signal 9"
        );
    }
}
