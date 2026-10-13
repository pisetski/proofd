mod cli_provider;
mod cli_resolver;
mod openai_compat;
mod sanitize;

use crate::config::{Config, ProviderKind};
use std::time::Duration;
use thiserror::Error;

pub use cli_provider::{cli_sandbox_dir, ClaudeCli, OpencodeCli};
pub use cli_resolver::resolve_cli;
pub use openai_compat::OpenAICompat;
pub use sanitize::{sanitize_output, strip_session_headers};

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

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError>;
}

pub fn from_config(cfg: &Config) -> ProviderImpl {
    match cfg.provider {
        ProviderKind::Claude => ProviderImpl::Claude(ClaudeCli {
            model: cfg.model.clone(),
            bin: cfg.provider_bin.clone(),
        }),
        ProviderKind::Opencode => ProviderImpl::Opencode(OpencodeCli {
            model: cfg.model.clone(),
            bin: cfg.provider_bin.clone(),
        }),
        ProviderKind::OpenAICompat => ProviderImpl::OpenAI(OpenAICompat {
            model: cfg.model.clone(),
            base_url: cfg.base_url.clone().unwrap_or_default(),
            client: reqwest::Client::new(),
        }),
    }
}

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

pub async fn complete_with_timeout<P: Provider>(
    provider: &P,
    system: &str,
    user: &str,
    timeout: Duration,
) -> Result<String, ProviderError> {
    tokio::time::timeout(timeout, provider.complete(system, user))
        .await
        .unwrap_or(Err(ProviderError::Timeout(timeout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SlowProvider;
    #[async_trait::async_trait]
    impl Provider for SlowProvider {
        async fn complete(&self, _s: &str, _u: &str) -> Result<String, ProviderError> {
            tokio::time::sleep(Duration::from_millis(300)).await;
            Ok("late".to_string())
        }
    }

    struct EchoProvider(String);
    #[async_trait::async_trait]
    impl Provider for EchoProvider {
        async fn complete(&self, _s: &str, _u: &str) -> Result<String, ProviderError> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn mocked_provider_timeout_path() {
        let err = complete_with_timeout(&SlowProvider, "s", "u", Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::Timeout(_)), "got: {err}");
    }

    #[tokio::test]
    async fn mocked_provider_success_path_returns_output_as_is() {
        let p = EchoProvider("  polished  ".to_string());
        let out = complete_with_timeout(&p, "s", "u", Duration::from_secs(2)).await;
        assert_eq!(out.unwrap(), "  polished  ");
    }
}
