mod claude;
mod cli_process;
mod cli_resolver;
mod openai_compat;
mod opencode;
mod opencode_sessions;

use crate::config::{Config, ProviderKind};
use claude::ClaudeCli;
use openai_compat::OpenAICompat;
use opencode::OpencodeCli;
use std::time::Duration;
use thiserror::Error;

pub(crate) use cli_resolver::resolve_cli;
pub use opencode_sessions::{FORGET_SESSIONS_FLAG, forget_sessions};

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider timed out after {0:?}")]
    Timeout(Duration),
    #[error("provider failed: {0}")]
    Failed(String),
    #[error("transport error: {0}")]
    Transport(String),
}

pub trait Provider: Send + Sync {
    fn complete(
        &self,
        system: &str,
        user: &str,
    ) -> impl Future<Output = Result<String, ProviderError>> + Send;
}

#[derive(Debug)]
pub enum AnyProvider {
    Claude(ClaudeCli),
    Opencode(OpencodeCli),
    OpenAICompat(OpenAICompat),
}

impl AnyProvider {
    pub fn from_config(cfg: &Config) -> Self {
        match cfg.provider {
            ProviderKind::Claude => Self::Claude(ClaudeCli {
                model: cfg.model.clone(),
                bin: cfg.provider_bin.clone(),
            }),
            ProviderKind::Opencode => Self::Opencode(OpencodeCli {
                model: cfg.model.clone(),
                bin: cfg.provider_bin.clone(),
            }),
            ProviderKind::OpenAICompat => Self::OpenAICompat(OpenAICompat {
                model: cfg.model.clone(),
                base_url: cfg.base_url.clone().unwrap_or_default(),
                client: reqwest::Client::new(),
            }),
        }
    }
}

impl Provider for AnyProvider {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        match self {
            Self::Claude(claude) => claude.complete(system, user).await,
            Self::Opencode(opencode) => opencode.complete(system, user).await,
            Self::OpenAICompat(server) => server.complete(system, user).await,
        }
    }
}
