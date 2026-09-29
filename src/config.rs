use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ProviderKind {
    #[default]
    #[serde(rename = "claude", alias = "Claude")]
    Claude,
    #[serde(rename = "opencode", alias = "Opencode", alias = "openCode")]
    Opencode,
    #[serde(
        rename = "openai-compat",
        alias = "openai_compat",
        alias = "openai",
        alias = "OpenAICompat"
    )]
    OpenAICompat,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Claude => write!(f, "claude"),
            Self::Opencode => write!(f, "opencode"),
            Self::OpenAICompat => write!(f, "openai-compat"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub provider: ProviderKind,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    /// Optional absolute path to the provider CLI (`claude` / `opencode`).
    /// Unset means PATH lookup plus common macOS locations
    /// (`/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin`, ...).
    /// Needed because LaunchAgents run with a minimal PATH that usually
    /// lacks Homebrew / cargo / `~/.local/bin`.
    #[serde(default)]
    pub provider_bin: Option<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_max_input_chars")]
    pub max_input_chars: usize,
}

fn default_model() -> String {
    "haiku".to_string()
}

fn default_timeout_secs() -> u64 {
    15
}

fn default_hotkey() -> String {
    "Ctrl-Alt-P".to_string()
}

fn default_max_input_chars() -> usize {
    12_000
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: ProviderKind::default(),
            model: default_model(),
            base_url: None,
            provider_bin: None,
            timeout_secs: default_timeout_secs(),
            hotkey: default_hotkey(),
            max_input_chars: default_max_input_chars(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid config at {path}: {msg}")]
    Invalid { path: String, msg: String },
    #[error("failed to read config at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

pub fn candidate_paths(filename: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push_unique = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    // 1. Explicit XDG location, honored on all platforms (including macOS
    //    where `dirs::config_dir()` ignores it).
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let xdg = PathBuf::from(xdg);
        if !xdg.as_os_str().is_empty() {
            push_unique(xdg.join("proofd").join(filename));
        }
    }
    // 2. Plain `~/.config` fallback so macOS reads the documented path.
    if let Some(home) = dirs::home_dir() {
        push_unique(home.join(".config").join("proofd").join(filename));
    }
    // 3. Platform-native dir (`~/Library/Application Support/...` on macOS).
    //    Kept for backward compat with installs that already use it.
    if let Some(d) = dirs::config_dir() {
        push_unique(d.join("proofd").join(filename));
    }
    out
}

pub fn config_candidates() -> Vec<PathBuf> {
    candidate_paths("config.toml")
}

pub fn config_path() -> Option<PathBuf> {
    let candidates = config_candidates();
    // Prefer the first file that actually exists so `~/.config` works on
    // macOS while installs using Application Support keep working.
    // With no file present, return the preferred location (first candidate)
    // so callers/error messages point at the documented path.
    candidates
        .iter()
        .find(|p| p.exists())
        .or_else(|| candidates.first())
        .cloned()
}

pub fn config_example() -> &'static str {
    include_str!("../config.example.toml")
}

/// Load config from `path`. `None` path tries [`config_candidates`] in order
/// (`$XDG_CONFIG_HOME`, then `~/.config`, then platform-native dir) and uses
/// the first file that exists.
/// Missing file returns defaults; invalid file returns a clear error.
pub fn load_from_path(path: Option<PathBuf>) -> Result<Config, ConfigError> {
    let path = path.or_else(config_path);
    let Some(path) = path else {
        return Ok(Config::default());
    };
    if !path.exists() {
        return Ok(Config::default());
    }
    let contents = std::fs::read_to_string(&path).map_err(|e| ConfigError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    parse_toml(&contents, &path.display().to_string())
}

/// Parse TOML contents, used by loader and tests.
pub fn parse_toml(contents: &str, path_for_errors: &str) -> Result<Config, ConfigError> {
    let mut cfg: Config = toml::from_str(contents).map_err(|e| ConfigError::Invalid {
        path: path_for_errors.to_string(),
        msg: e.to_string(),
    })?;
    validate(&mut cfg, path_for_errors)?;
    Ok(cfg)
}

fn validate(cfg: &mut Config, path_for_errors: &str) -> Result<(), ConfigError> {
    let invalid = |msg: &str| ConfigError::Invalid {
        path: path_for_errors.to_string(),
        msg: msg.to_string(),
    };
    if cfg.model.trim().is_empty() {
        return Err(invalid("`model` must not be empty"));
    }
    if cfg.timeout_secs == 0 || cfg.timeout_secs > 600 {
        return Err(invalid("`timeout_secs` must be in 1..=600"));
    }
    if cfg.hotkey.trim().is_empty() {
        return Err(invalid("`hotkey` must not be empty"));
    }
    if cfg.max_input_chars == 0 {
        return Err(invalid("`max_input_chars` must be > 0"));
    }
    if let Some(bin) = cfg.provider_bin.as_ref() {
        if bin.trim().is_empty() {
            return Err(invalid("`provider_bin` must not be empty"));
        }
    }
    if cfg.provider == ProviderKind::OpenAICompat {
        match &cfg.base_url {
            Some(u) if !u.trim().is_empty() => {}
            _ => {
                return Err(invalid(
                    "`base_url` is required when provider = \"openai-compat\"",
                ));
            }
        }
    }
    // Normalize trailing slash on base_url.
    if let Some(u) = cfg.base_url.take() {
        let trimmed = u.trim().trim_end_matches('/').to_string();
        cfg.base_url = Some(trimmed);
    }
    // Normalize provider_bin whitespace; treat empty as unset (validated above).
    if let Some(b) = cfg.provider_bin.take() {
        let trimmed = b.trim().to_string();
        if !trimmed.is_empty() {
            cfg.provider_bin = Some(trimmed);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn missing_file_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("config.toml");
        let cfg = load_from_path(Some(missing)).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.provider, ProviderKind::Claude);
        assert_eq!(cfg.model, "haiku");
        assert_eq!(cfg.timeout_secs, 15);
        assert_eq!(cfg.hotkey, "Ctrl-Alt-P");
    }

    #[test]
    fn valid_config_parses() {
        let toml = r#"
provider = "openai-compat"
model = "llama3"
base_url = "http://localhost:11434/v1/"
timeout_secs = 20
hotkey = "Ctrl-Alt-P"
max_input_chars = 2000
"#;
        let cfg = parse_toml(toml, "test").unwrap();
        assert_eq!(cfg.provider, ProviderKind::OpenAICompat);
        assert_eq!(cfg.model, "llama3");
        // Trailing slash normalized.
        assert_eq!(cfg.base_url.as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(cfg.timeout_secs, 20);
    }

    #[test]
    fn invalid_config_errors_clearly() {
        let bad = "provider = [unclosed\n";
        let err = parse_toml(bad, "/tmp/fake.toml").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/tmp/fake.toml"), "got: {msg}");
        assert!(msg.contains("invalid config"), "got: {msg}");
    }

    #[test]
    fn invalid_values_rejected() {
        let err = parse_toml("timeout_secs = 0", "p").unwrap_err().to_string();
        assert!(err.contains("timeout_secs"), "got: {err}");
        let err = parse_toml("provider = \"openai-compat\"\nmodel = \"x\"", "p")
            .unwrap_err()
            .to_string();
        assert!(err.contains("base_url"), "got: {err}");
    }

    #[test]
    fn load_invalid_file_errors() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "timeout_secs = 0").unwrap();
        let err = load_from_path(Some(f.path().to_path_buf()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid config"), "got: {err}");
    }

    #[test]
    fn provider_bin_optional_defaults_to_none() {
        let cfg = parse_toml("provider = \"opencode\"\nmodel = \"x/y\"", "p").unwrap();
        assert_eq!(cfg.provider_bin, None);
        let cfg = parse_toml("provider_bin = \"/opt/homebrew/bin/opencode\"", "p").unwrap();
        assert_eq!(
            cfg.provider_bin.as_deref(),
            Some("/opt/homebrew/bin/opencode")
        );
    }
}
