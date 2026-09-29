use crate::config::Config;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider timed out after {0:?}")]
    Timeout(Duration),
    #[error("provider returned empty output")]
    EmptyOutput,
    #[error("provider failed: {0}")]
    Failed(String),
    #[error("transport error: {0}")]
    Transport(String),
}

/// Shared sanitizer:
/// - trim surrounding whitespace
/// - strip one surrounding code fence if the entire response is fenced
/// - preserve inner whitespace
/// - empty result is an error; caller must never paste empty output.
pub fn sanitize_output(s: &str) -> Result<String, ProviderError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(ProviderError::EmptyOutput);
    }
    let stripped = strip_single_fence(trimmed);
    let out = stripped.trim();
    if out.is_empty() {
        return Err(ProviderError::EmptyOutput);
    }
    Ok(out.to_string())
}

fn strip_single_fence(s: &str) -> &str {
    let t = s.trim();
    if !t.starts_with("```") {
        return s.trim();
    }
    // Find first fence line end.
    let Some(first_newline) = t.find('\n') else {
        return s.trim();
    };
    // Must end with closing fence.
    let Some(close_idx) = t.rfind("```") else {
        return s.trim();
    };
    if close_idx <= first_newline {
        return s.trim();
    }
    let inner = &t[first_newline + 1..close_idx];
    // Only strip if the fences wrap the whole response (nothing but
    // whitespace after closing fence, which trim already ensures).
    inner.trim_matches('\n')
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError>;
}

/// Build the configured provider from config.
pub fn from_config(cfg: &Config) -> ProviderImpl {
    match cfg.provider {
        crate::config::ProviderKind::Claude => ProviderImpl::Claude(ClaudeCli {
            model: cfg.model.clone(),
            bin: cfg.provider_bin.clone(),
        }),
        crate::config::ProviderKind::Opencode => ProviderImpl::Opencode(OpencodeCli {
            model: cfg.model.clone(),
            bin: cfg.provider_bin.clone(),
        }),
        crate::config::ProviderKind::OpenAICompat => ProviderImpl::OpenAI(OpenAICompat {
            model: cfg.model.clone(),
            base_url: cfg.base_url.clone().unwrap_or_default(),
            client: reqwest::Client::new(),
        }),
    }
}

/// Enum wrapper so callers don't need `Arc<dyn Provider>` if they prefer.
/// Still implements [`Provider`] via delegation (Option A + B hybrid).
#[derive(Debug)]
pub enum ProviderImpl {
    Claude(ClaudeCli),
    Opencode(OpencodeCli),
    OpenAI(OpenAICompat),
}

#[async_trait::async_trait]
impl Provider for ProviderImpl {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        match self {
            Self::Claude(p) => p.complete(system, user).await,
            Self::Opencode(p) => p.complete(system, user).await,
            Self::OpenAI(p) => p.complete(system, user).await,
        }
    }
}

/// Resolve a provider CLI binary.
///
/// LaunchAgents run with a minimal `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`),
/// so a bare `Command::new("opencode")` fails with ENOENT even when the
/// binary works fine in your terminal (e.g. `/opt/homebrew/bin/opencode`).
///
/// Order: explicit `provider_bin` override (may be absolute or `~/`-prefixed)
/// first, then each `PATH` entry, then common macOS locations.
pub fn resolve_cli(explicit: Option<&str>, name: &str) -> Result<String, ProviderError> {
    if let Some(raw) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        let expanded = expand_tilde(raw);
        // Absolute/relative path containing a slash: use directly if it exists.
        if expanded.contains('/') {
            if std::path::Path::new(&expanded).is_file() {
                return Ok(expanded);
            }
            return Err(ProviderError::Transport(format!(
                "provider_bin {expanded:?} not found; check `provider_bin` in config"
            )));
        }
        // Bare name override: still try PATH first.
        if let Some(found) = find_on_path(&expanded) {
            return Ok(found);
        }
        return Err(ProviderError::Transport(format!(
            "provider_bin {expanded:?} not found on PATH ({})",
            path_for_error()
        )));
    }
    if let Some(found) = find_on_path(name) {
        return Ok(found);
    }
    for dir in fallback_dirs() {
        let cand = std::path::Path::new(&dir).join(name);
        if cand.is_file() {
            return Ok(cand.display().to_string());
        }
    }
    Err(ProviderError::Transport(format!(
        "spawn {name}: not found on PATH ({}) nor in {} — \
         set `provider_bin` in config (e.g. `provider_bin = \"/opt/homebrew/bin/{name}\"`) \
         or add an EnvironmentVariables PATH to your LaunchAgent plist",
        path_for_error(),
        fallback_dirs().join(", "),
    )))
}

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    s.to_string()
}

fn find_on_path(name: &str) -> Option<String> {
    // Absolute path shortcut.
    if name.contains('/') {
        let expanded = expand_tilde(name);
        let p = std::path::Path::new(&expanded);
        if p.is_file() {
            return Some(p.display().to_string());
        }
        return None;
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand.display().to_string());
        }
    }
    None
}

fn fallback_dirs() -> Vec<String> {
    let mut dirs = vec![
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/opt/local/bin".to_string(),
        "/usr/bin".to_string(),
        "/bin".to_string(),
    ];
    if let Some(home) = dirs::home_dir() {
        for sub in [".cargo/bin", ".local/bin", ".opencode/bin", ".bun/bin"] {
            dirs.push(home.join(sub).display().to_string());
        }
    }
    dirs
}

fn path_for_error() -> String {
    std::env::var("PATH").unwrap_or_else(|_| "<unset>".to_string())
}

/// Claude CLI provider.
/// Spawns `claude -p --model <model> --output-format text`, prompt via stdin.
/// Do not invent flags: before adding --no-session-persistence/--tools etc.,
/// verify with `claude --help`.
#[derive(Debug, Clone)]
pub struct ClaudeCli {
    pub model: String,
    /// Optional explicit binary path (from `provider_bin`). `None` means
    /// PATH lookup plus common macOS locations.
    pub bin: Option<String>,
}

#[async_trait::async_trait]
impl Provider for ClaudeCli {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        use tokio::io::AsyncWriteExt;
        let full_prompt = format!("{system}\n\n{user}");
        let bin = resolve_cli(self.bin.as_deref(), "claude")?;
        let mut child = tokio::process::Command::new(&bin)
            .args(["-p", "--model", &self.model, "--output-format", "text"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ProviderError::Transport(format!("spawn claude ({bin}): {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(full_prompt.as_bytes())
                .await
                .map_err(|e| ProviderError::Transport(format!("write claude stdin: {e}")))?;
        }
        let out = child
            .wait_with_output()
            .await
            .map_err(|e| ProviderError::Transport(format!("wait claude: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(ProviderError::Failed(format!(
                "claude exited {}: {}",
                out.status,
                stderr.trim()
            )));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        sanitize_output(&text)
    }
}

/// Opencode CLI provider.
/// Syntax: `opencode run --model <provider/model> --format default`,
/// prompt via stdin. NOTE: `--model` needs the `provider/model` form
/// (e.g. `opencode/mimo-v2.6-flash-free`); a bare model name fails server-side.
#[derive(Debug, Clone)]
pub struct OpencodeCli {
    pub model: String,
    /// Optional explicit binary path (from `provider_bin`). `None` means
    /// PATH lookup plus common macOS locations.
    pub bin: Option<String>,
}

#[async_trait::async_trait]
impl Provider for OpencodeCli {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        use tokio::io::AsyncWriteExt;
        let full_prompt = format!("{system}\n\n{user}");
        let bin = resolve_cli(self.bin.as_deref(), "opencode")?;
        let mut child = tokio::process::Command::new(&bin)
            .args(["run", "--model", &self.model, "--format", "default"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ProviderError::Transport(format!("spawn opencode ({bin}): {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(full_prompt.as_bytes())
                .await
                .map_err(|e| ProviderError::Transport(format!("write opencode stdin: {e}")))?;
        }
        let out = child
            .wait_with_output()
            .await
            .map_err(|e| ProviderError::Transport(format!("wait opencode: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(ProviderError::Failed(format!(
                "opencode exited {}: {}",
                out.status,
                stderr.trim()
            )));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let stripped = strip_session_headers(&text);
        sanitize_output(&stripped)
    }
}

/// Strip known session/thinking headers only if observed in real output.
/// Conservative: drop leading lines that look like session metadata.
pub fn strip_session_headers(s: &str) -> String {
    let mut lines = s.lines();
    let mut skipped_prefix = false;
    let mut out: Vec<&str> = Vec::new();
    for line in lines.by_ref() {
        let t = line.trim();
        if !skipped_prefix
            && (!t.is_empty()
                && (t.starts_with("Session:")
                    || t.starts_with("Thinking:")
                    || t.starts_with("model:")
                    || t.starts_with("opencode:")))
        {
            continue;
        }
        skipped_prefix = true;
        out.push(line);
    }
    out.join("\n")
}

/// OpenAI-compatible HTTP provider.
/// POST {base_url}/chat/completions {model, messages, temperature: 0.2}.
#[derive(Debug, Clone)]
pub struct OpenAICompat {
    pub model: String,
    pub base_url: String,
    pub client: reqwest::Client,
}

#[async_trait::async_trait]
impl Provider for OpenAICompat {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": 0.2,
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Transport(format!("openai-compat request: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Failed(format!(
                "openai-compat {status}: {}",
                text.trim()
            )));
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Transport(format!("openai-compat decode: {e}")))?;
        let content = v
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        sanitize_output(&content)
    }
}

/// Run a provider call with timeout. Caller enforces via tokio::time::timeout.
pub async fn complete_with_timeout<P: Provider>(
    provider: &P,
    system: &str,
    user: &str,
    timeout: Duration,
) -> Result<String, ProviderError> {
    match tokio::time::timeout(timeout, provider.complete(system, user)).await {
        Ok(r) => r,
        Err(_) => Err(ProviderError::Timeout(timeout)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn sanitize_trims_normal_output() {
        assert_eq!(sanitize_output("  hello world  \n").unwrap(), "hello world");
        assert_eq!(
            sanitize_output("\n\n  multi\n  line  \n\n").unwrap(),
            "multi\n  line"
        );
    }

    #[test]
    fn sanitize_strips_single_accidental_fence() {
        let fenced = "```\nhello world\n```";
        assert_eq!(sanitize_output(fenced).unwrap(), "hello world");
        let fenced_lang = "```text\nhello\nworld\n```";
        assert_eq!(sanitize_output(fenced_lang).unwrap(), "hello\nworld");
        // Inner whitespace preserved.
        let inner = "```\n  indented\n\tcode  \n```";
        assert_eq!(sanitize_output(inner).unwrap(), "indented\n\tcode");
    }

    #[test]
    fn sanitize_rejects_empty() {
        assert!(matches!(
            sanitize_output("   \n\t  "),
            Err(ProviderError::EmptyOutput)
        ));
        assert!(matches!(
            sanitize_output("```\n   \n```"),
            Err(ProviderError::EmptyOutput)
        ));
        // Must never paste empty: callers check this error.
    }

    #[test]
    fn sanitize_does_not_strip_partial_fences() {
        // Fence present in input text but not wrapping whole response: keep.
        let s = "Use ```rust for code";
        assert_eq!(sanitize_output(s).unwrap(), "Use ```rust for code");
    }

    struct SlowProvider;
    #[async_trait::async_trait]
    impl Provider for SlowProvider {
        async fn complete(&self, _s: &str, _u: &str) -> Result<String, ProviderError> {
            tokio::time::sleep(Duration::from_millis(300)).await;
            Ok("late".to_string())
        }
    }

    struct EchoProvider(pub String);
    #[async_trait::async_trait]
    impl Provider for EchoProvider {
        async fn complete(&self, _s: &str, _u: &str) -> Result<String, ProviderError> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn mocked_provider_timeout_path() {
        let p = SlowProvider;
        let err = complete_with_timeout(&p, "s", "u", Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::Timeout(_)), "got: {err}");
    }

    #[tokio::test]
    async fn mocked_provider_success_path() {
        let p = EchoProvider("  polished  ".to_string());
        let out = complete_with_timeout(&p, "s", "u", Duration::from_secs(2)).await;
        // Note: mock returns raw; complete_with_timeout returns provider output as-is.
        assert_eq!(out.unwrap(), "  polished  ");
    }

    #[test]
    fn resolve_explicit_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("my-opencode");
        std::fs::write(&bin, "#!/bin/sh\necho hi\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
        }
        let got = resolve_cli(Some(bin.to_str().unwrap()), "opencode").unwrap();
        assert_eq!(got, bin.display().to_string());
    }

    #[test]
    fn resolve_explicit_missing_errors_clearly() {
        let err = resolve_cli(Some("/nonexistent-xyz/proofd-opencode"), "opencode")
            .unwrap_err()
            .to_string();
        assert!(err.contains("provider_bin"), "got: {err}");
    }

    #[test]
    fn resolve_missing_binary_mentions_provider_bin() {
        let err = resolve_cli(None, "definitely-not-a-real-binary-xyz")
            .unwrap_err()
            .to_string();
        assert!(err.contains("provider_bin"), "got: {err}");
        assert!(err.contains("LaunchAgent"), "got: {err}");
    }
}
