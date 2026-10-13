use super::cli_resolver::resolve_cli;
use super::sanitize::{sanitize_output, strip_session_headers};
use super::{Provider, ProviderError};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct ClaudeCli {
    pub model: String,
    pub bin: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OpencodeCli {
    pub model: String,
    pub bin: Option<String>,
}

#[async_trait::async_trait]
impl Provider for ClaudeCli {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        let args = claude_args(&self.model, system);
        let stdout = run_cli(
            "claude",
            self.bin.as_deref(),
            &args,
            user,
            cli_sandbox_dir().as_deref(),
        )
        .await?;
        sanitize_output(&stdout)
    }
}

#[async_trait::async_trait]
impl Provider for OpencodeCli {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        let mut args = ["run", "--model", &self.model, "--format", "default"]
            .map(String::from)
            .to_vec();
        let sandbox = cli_sandbox_dir();
        if let Some(dir) = &sandbox {
            args.extend(["--dir".to_string(), dir.display().to_string()]);
        }
        let prompt = format!("{system}\n\n{user}");
        let stdout = run_cli(
            "opencode",
            self.bin.as_deref(),
            &args,
            &prompt,
            sandbox.as_deref(),
        )
        .await?;
        sanitize_output(&strip_session_headers(&stdout))
    }
}

fn claude_args(model: &str, system: &str) -> Vec<String> {
    [
        "-p",
        "--model",
        model,
        "--output-format",
        "text",
        "--tools",
        "",
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

async fn run_cli(
    name: &str,
    explicit_bin: Option<&str>,
    args: &[String],
    stdin_text: &str,
    cwd: Option<&Path>,
) -> Result<String, ProviderError> {
    let total = Instant::now();
    let bin = resolve_cli(explicit_bin, name)?;
    let resolve_ms = total.elapsed().as_millis();
    let mut command = Command::new(&bin);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let mut child = command
        .spawn()
        .map_err(|e| ProviderError::Transport(format!("spawn {name} ({bin}): {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_text.as_bytes())
            .await
            .map_err(|e| ProviderError::Transport(format!("write {name} stdin: {e}")))?;
    }
    let wait = Instant::now();
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| ProviderError::Transport(format!("wait {name}: {e}")))?;
    tracing::debug!(
        "{name} timings resolve={resolve_ms}ms wait={}ms total={}ms",
        wait.elapsed().as_millis(),
        total.elapsed().as_millis(),
    );
    if !output.status.success() {
        return Err(ProviderError::Failed(format!(
            "{name} exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
        assert!(args.contains(&"--no-session-persistence".to_string()));
        assert!(args.contains(&"--disable-slash-commands".to_string()));
    }
}
