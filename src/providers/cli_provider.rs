use super::cli_resolver::resolve_cli;
use super::sanitize::{sanitize_output, strip_session_headers};
use super::{Provider, ProviderError};
use std::path::PathBuf;
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
        let args = ["-p", "--model", &self.model, "--output-format", "text"].map(String::from);
        let stdout = run_cli("claude", self.bin.as_deref(), &args, system, user).await?;
        sanitize_output(&stdout)
    }
}

#[async_trait::async_trait]
impl Provider for OpencodeCli {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        let mut args = ["run", "--model", &self.model, "--format", "default"]
            .map(String::from)
            .to_vec();
        if let Some(dir) = opencode_sandbox_dir() {
            args.extend(["--dir".to_string(), dir.display().to_string()]);
        }
        let stdout = run_cli("opencode", self.bin.as_deref(), &args, system, user).await?;
        sanitize_output(&strip_session_headers(&stdout))
    }
}

async fn run_cli(
    name: &str,
    explicit_bin: Option<&str>,
    args: &[String],
    system: &str,
    user: &str,
) -> Result<String, ProviderError> {
    let total = Instant::now();
    let bin = resolve_cli(explicit_bin, name)?;
    let resolve_ms = total.elapsed().as_millis();
    let mut child = Command::new(&bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ProviderError::Transport(format!("spawn {name} ({bin}): {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(format!("{system}\n\n{user}").as_bytes())
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

// `opencode run` is agentic and picks up skills from its cwd, which turned revisions into essays.
pub fn opencode_sandbox_dir() -> Option<PathBuf> {
    static SANDBOX: OnceLock<Option<PathBuf>> = OnceLock::new();
    SANDBOX
        .get_or_init(|| {
            let base = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
            let dir = base.join("proofd").join("opencode-sandbox");
            match std::fs::create_dir_all(&dir) {
                Ok(()) => Some(dir),
                Err(e) => {
                    tracing::warn!("opencode sandbox unavailable at {}: {e}", dir.display());
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
    fn opencode_sandbox_is_stable_and_empty_of_skills() {
        let a = opencode_sandbox_dir().expect("sandbox must exist in test env");
        let b = opencode_sandbox_dir().expect("sandbox must be cached");
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
}
