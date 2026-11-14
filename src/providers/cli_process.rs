use super::ProviderError;
use std::io::ErrorKind;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::OnceLock;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, Command};

pub(super) struct CliInvocation<'a> {
    pub name: &'a str,
    pub program: &'a str,
    pub args: &'a [String],
    pub env: &'a [(&'a str, &'a str)],
    pub stdin_text: &'a str,
    pub cwd: Option<&'a Path>,
    pub fatal_stderr_line: fn(&str) -> Option<String>,
}

pub(super) fn no_fatal_stderr_lines(_line: &str) -> Option<String> {
    None
}

pub(super) async fn run_cli(cli: CliInvocation<'_>) -> Result<String, ProviderError> {
    let name = cli.name;
    let started = Instant::now();
    let mut command = Command::new(cli.program);
    command
        .args(cli.args)
        .envs(cli.env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = cli.cwd {
        command.current_dir(dir);
    }
    let mut child = command
        .spawn()
        .map_err(|e| transport_error(&format!("spawn {name} ({})", cli.program), e))?;
    let stdin = child.stdin.take().expect("stdin is piped");
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    let (_, stdout, stderr, status) = tokio::try_join!(
        write_then_close_tolerating_early_exit(name, stdin, cli.stdin_text),
        read_to_end(name, "stdout", stdout),
        collect_stderr_failing_on_fatal_line(name, stderr, cli.fatal_stderr_line),
        async {
            child
                .wait()
                .await
                .map_err(|e| transport_error(&format!("wait {name}"), e))
        },
    )?;
    tracing::debug!("{name} took {}ms", started.elapsed().as_millis());
    if !status.success() {
        return Err(ProviderError::Failed(exit_failure(name, status, &stderr)));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

fn transport_error(action: &str, error: std::io::Error) -> ProviderError {
    ProviderError::Transport(format!("{action}: {error}"))
}

async fn write_then_close_tolerating_early_exit(
    name: &str,
    mut stdin: ChildStdin,
    text: &str,
) -> Result<(), ProviderError> {
    let written = stdin.write_all(text.as_bytes()).await;
    drop(stdin);
    match written {
        Err(e) if e.kind() != ErrorKind::BrokenPipe => {
            Err(transport_error(&format!("write {name} stdin"), e))
        }
        _ => Ok(()),
    }
}

async fn read_to_end(
    name: &str,
    stream_name: &str,
    mut stream: impl AsyncRead + Unpin,
) -> Result<Vec<u8>, ProviderError> {
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| transport_error(&format!("read {name} {stream_name}"), e))?;
    Ok(bytes)
}

async fn collect_stderr_failing_on_fatal_line(
    name: &str,
    stderr: ChildStderr,
    fatal_stderr_line: fn(&str) -> Option<String>,
) -> Result<String, ProviderError> {
    let mut reader = BufReader::new(stderr);
    let mut collected = String::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .await
            .map_err(|e| transport_error(&format!("read {name} stderr"), e))?;
        if read == 0 {
            return Ok(collected);
        }
        let text = String::from_utf8_lossy(&line);
        if let Some(reason) = fatal_stderr_line(&text) {
            return Err(ProviderError::Failed(format!("{name}: {reason}")));
        }
        collected.push_str(&text);
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

pub(super) fn cwd_without_project_context() -> Option<PathBuf> {
    static EMPTY_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    EMPTY_DIR
        .get_or_init(|| {
            let cache = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
            let dir = cache.join("proofd").join("cli-sandbox");
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
    fn cli_cwd_is_stable_and_free_of_skills_and_project_files() {
        let a = cwd_without_project_context().expect("sandbox must exist in test env");
        let b = cwd_without_project_context().expect("sandbox must be cached");
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
