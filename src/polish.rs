use crate::config::Config;
use crate::prompt::build_revised_only;
use crate::providers::{complete_with_timeout, Provider};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolishError {
    #[error("no text to polish")]
    NoText,
    #[error("input too large ({0} chars > max {1})")]
    TooLarge(usize, usize),
    #[error("provider returned empty output")]
    EmptyOutput,
    #[error("provider error: {0}")]
    Provider(String),
}

pub async fn polish_text<P: Provider>(
    input: &str,
    config: &Config,
    provider: &P,
) -> Result<String, PolishError> {
    if input.trim().is_empty() {
        return Err(PolishError::NoText);
    }
    let input_chars = input.chars().count();
    if input_chars > config.max_input_chars {
        return Err(PolishError::TooLarge(input_chars, config.max_input_chars));
    }

    let (system, user) = build_revised_only(input);
    let timeout = Duration::from_secs(config.timeout_secs);
    let revised = complete_with_timeout(provider, &system, &user, timeout)
        .await
        .map_err(|e| PolishError::Provider(e.to_string()))?;
    if revised.trim().is_empty() {
        return Err(PolishError::EmptyOutput);
    }
    Ok(revised)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ProviderError;

    struct EchoProvider(String);
    #[async_trait::async_trait]
    impl Provider for EchoProvider {
        async fn complete(&self, _s: &str, _u: &str) -> Result<String, ProviderError> {
            Ok(self.0.clone())
        }
    }

    struct FailProvider;
    #[async_trait::async_trait]
    impl Provider for FailProvider {
        async fn complete(&self, _s: &str, _u: &str) -> Result<String, ProviderError> {
            Err(ProviderError::Failed("boom".into()))
        }
    }

    fn test_config() -> Config {
        Config {
            timeout_secs: 5,
            max_input_chars: 100,
            ..Config::default()
        }
    }

    #[tokio::test]
    async fn returns_revised_text() {
        let revised = polish_text("helo", &test_config(), &EchoProvider("hello".into()))
            .await
            .unwrap();
        assert_eq!(revised, "hello");
    }

    #[tokio::test]
    async fn blank_input_never_reaches_provider() {
        let err = polish_text(" \n", &test_config(), &FailProvider)
            .await
            .unwrap_err();
        assert!(matches!(err, PolishError::NoText), "got: {err}");
    }

    #[tokio::test]
    async fn oversized_input_aborts_before_provider() {
        let err = polish_text(&"x".repeat(101), &test_config(), &FailProvider)
            .await
            .unwrap_err();
        assert!(matches!(err, PolishError::TooLarge(101, 100)), "got: {err}");
    }

    #[tokio::test]
    async fn input_limit_counts_chars_not_bytes() {
        polish_text(&"é".repeat(100), &test_config(), &EchoProvider("ok".into()))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn provider_failure_is_an_error() {
        let err = polish_text("text", &test_config(), &FailProvider)
            .await
            .unwrap_err();
        assert!(matches!(err, PolishError::Provider(_)), "got: {err}");
    }

    #[tokio::test]
    async fn blank_provider_output_is_an_error() {
        let err = polish_text("text", &test_config(), &EchoProvider("  \n".into()))
            .await
            .unwrap_err();
        assert!(matches!(err, PolishError::EmptyOutput), "got: {err}");
    }
}
