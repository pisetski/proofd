use crate::paths;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum ProviderKind {
    #[default]
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "opencode")]
    Opencode,
    #[serde(rename = "openai-compat")]
    OpenAICompat,
}

impl ProviderKind {
    fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Opencode => "opencode",
            Self::OpenAICompat => "openai-compat",
        }
    }

    fn default_model(self) -> Option<&'static str> {
        match self {
            Self::Claude => Some("haiku"),
            Self::Opencode => Some("opencode/deepseek-v4.1-flash-free"),
            Self::OpenAICompat => None,
        }
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

const CONFIG_FILE: &str = "config.toml";
const DEFAULT_TIMEOUT_SECS: u64 = 15;
const MAX_TIMEOUT_SECS: u64 = 600;
const DEFAULT_MAX_INPUT_CHARS: usize = 12_000;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub provider: ProviderKind,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub provider_bin: Option<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_input_chars")]
    pub max_input_chars: usize,
}

fn default_timeout_secs() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

fn default_max_input_chars() -> usize {
    DEFAULT_MAX_INPUT_CHARS
}

impl Default for Config {
    fn default() -> Self {
        Self::for_provider(ProviderKind::default())
    }
}

impl Config {
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    fn for_provider(provider: ProviderKind) -> Self {
        Self {
            provider,
            model: provider.default_model().unwrap_or_default().to_string(),
            base_url: None,
            provider_bin: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_input_chars: DEFAULT_MAX_INPUT_CHARS,
        }
    }

    fn for_installed_clis(is_installed: impl Fn(&str) -> bool) -> Self {
        if !is_installed("claude") && is_installed("opencode") {
            return Self::for_provider(ProviderKind::Opencode);
        }
        Self::default()
    }

    fn validated(mut self) -> Result<Self, String> {
        if self.model.trim().is_empty() {
            let Some(default_model) = self.provider.default_model() else {
                return Err(format!(
                    "`model` is required when provider = \"{}\"",
                    self.provider
                ));
            };
            self.model = default_model.to_string();
        }
        if !(1..=MAX_TIMEOUT_SECS).contains(&self.timeout_secs) {
            return Err(format!("`timeout_secs` must be in 1..={MAX_TIMEOUT_SECS}"));
        }
        if self.max_input_chars == 0 {
            return Err("`max_input_chars` must be > 0".to_string());
        }
        self.provider_bin = match self.provider_bin.map(|bin| bin.trim().to_string()) {
            Some(blank) if blank.is_empty() => {
                return Err("`provider_bin` must not be empty".to_string());
            }
            bin => bin,
        };
        self.base_url = self
            .base_url
            .map(|url| url.trim().trim_end_matches('/').to_string());
        if self.provider == ProviderKind::OpenAICompat
            && self.base_url.as_deref().is_none_or(str::is_empty)
        {
            return Err("`base_url` is required when provider = \"openai-compat\"".to_string());
        }
        Ok(self)
    }
}

fn cli_installed(name: &str) -> bool {
    crate::providers::resolve_cli(None, name).is_ok()
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

pub fn config_path() -> Option<PathBuf> {
    paths::existing_or_preferred_path(CONFIG_FILE)
}

pub fn load(path: Option<&Path>) -> Result<Config, ConfigError> {
    let Some(path) = path.filter(|path| path.exists()) else {
        return Ok(Config::for_installed_clis(cli_installed));
    };
    let contents = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.display().to_string(),
        source,
    })?;
    parse_toml(&contents, &path.display().to_string())
}

pub fn parse_toml(contents: &str, path_for_errors: &str) -> Result<Config, ConfigError> {
    let invalid = |msg: String| ConfigError::Invalid {
        path: path_for_errors.to_string(),
        msg,
    };
    let parsed: Config = toml::from_str(contents).map_err(|e| invalid(e.to_string()))?;
    parsed.validated().map_err(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn missing_file_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("config.toml");
        let cfg = load(Some(&missing)).unwrap();
        assert_eq!(cfg, Config::for_installed_clis(cli_installed));
        assert_eq!(cfg.timeout_secs, 15);
    }

    #[test]
    fn for_installed_clis_prefers_claude() {
        let cfg = Config::for_installed_clis(|_| true);
        assert_eq!(cfg.provider, ProviderKind::Claude);
        assert_eq!(cfg.model, "haiku");
        assert_eq!(Config::for_installed_clis(|_| false), Config::default());
    }

    #[test]
    fn for_installed_clis_falls_back_to_opencode_when_only_it_is_installed() {
        let cfg = Config::for_installed_clis(|name| name == "opencode");
        assert_eq!(cfg.provider, ProviderKind::Opencode);
        assert_eq!(cfg.model, "opencode/deepseek-v4.1-flash-free");
    }

    #[test]
    fn valid_config_parses() {
        let toml = r#"
provider = "openai-compat"
model = "llama3"
base_url = "http://localhost:11434/v1/"
timeout_secs = 20
max_input_chars = 2000
"#;
        let cfg = parse_toml(toml, "test").unwrap();
        assert_eq!(cfg.provider, ProviderKind::OpenAICompat);
        assert_eq!(cfg.model, "llama3");
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
    fn misspelled_key_is_rejected_instead_of_ignored() {
        let err = parse_toml("modle = \"sonnet\"", "p")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown field `modle`"), "got: {err}");
    }

    #[test]
    fn model_defaults_to_the_providers_own() {
        assert_eq!(parse_toml("", "p").unwrap().model, "haiku");
        let cfg = parse_toml("provider = \"opencode\"\nmodel = \" \"", "p").unwrap();
        assert_eq!(cfg.model, "opencode/deepseek-v4.1-flash-free");
    }

    #[test]
    fn openai_compat_requires_a_model() {
        let err = parse_toml(
            "provider = \"openai-compat\"\nbase_url = \"http://localhost:1234/v1\"",
            "p",
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("`model` is required when provider = \"openai-compat\""),
            "got: {err}"
        );
    }

    #[test]
    fn load_invalid_file_errors() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "timeout_secs = 0").unwrap();
        let err = load(Some(f.path())).unwrap_err().to_string();
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
