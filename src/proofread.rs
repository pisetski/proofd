use crate::config::Config;
use crate::prompt::build_revised_only;
use crate::providers::{Provider, complete_with_timeout, strip_wrapping_fence};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProofreadError {
    #[error("no text to proofread")]
    NoText,
    #[error("input too large ({0} chars > max {1})")]
    TooLarge(usize, usize),
    #[error("provider returned empty output")]
    EmptyOutput,
    #[error("provider error: {0}")]
    Provider(String),
}

pub async fn proofread_text<P: Provider>(
    input: &str,
    config: &Config,
    provider: &P,
) -> Result<String, ProofreadError> {
    if input.trim().is_empty() {
        return Err(ProofreadError::NoText);
    }
    let input_chars = input.chars().count();
    if input_chars > config.max_input_chars {
        return Err(ProofreadError::TooLarge(
            input_chars,
            config.max_input_chars,
        ));
    }

    let (system, user) = build_revised_only(input);
    let timeout = Duration::from_secs(config.timeout_secs);
    let revised = complete_with_timeout(provider, &system, &user, timeout)
        .await
        .map_err(|e| ProofreadError::Provider(e.to_string()))?;
    let revised = strip_wrapping_fence(&revised, input);
    if revised.is_empty() {
        return Err(ProofreadError::EmptyOutput);
    }
    Ok(with_surrounding_whitespace_of(input, revised))
}

// The result replaces the selection, so its edges must match: a triple-clicked line
// keeps its newline and an indented line keeps its indent.
fn with_surrounding_whitespace_of(input: &str, revised: &str) -> String {
    let leading = &input[..input.len() - input.trim_start().len()];
    let trailing = &input[input.trim_end().len()..];
    format!("{leading}{}{trailing}", revised.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ProviderError;

    struct EchoProvider(String);
    impl Provider for EchoProvider {
        async fn complete(&self, _s: &str, _u: &str) -> Result<String, ProviderError> {
            Ok(self.0.clone())
        }
    }

    struct FailProvider;
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
        let revised = proofread_text("helo", &test_config(), &EchoProvider("hello".into()))
            .await
            .unwrap();
        assert_eq!(revised, "hello");
    }

    #[tokio::test]
    async fn keeps_the_selections_surrounding_whitespace() {
        let revised = proofread_text(
            "    - fixd item\n",
            &test_config(),
            &EchoProvider("- fixed item".into()),
        )
        .await
        .unwrap();
        assert_eq!(revised, "    - fixed item\n");
    }

    #[tokio::test]
    async fn does_not_double_an_indent_the_model_kept() {
        let revised = proofread_text(
            "    - fixd item\n",
            &test_config(),
            &EchoProvider("    - fixed item".into()),
        )
        .await
        .unwrap();
        assert_eq!(revised, "    - fixed item\n");
    }

    #[tokio::test]
    async fn keeps_crlf_and_non_ascii_edges() {
        let config = test_config();
        let provider = EchoProvider("hello".into());
        let crlf = proofread_text("helo\r\n", &config, &provider).await;
        assert_eq!(crlf.unwrap(), "hello\r\n");
        let nbsp = proofread_text("\u{a0}helo\u{3000}", &config, &provider).await;
        assert_eq!(nbsp.unwrap(), "\u{a0}hello\u{3000}");
    }

    #[tokio::test]
    async fn strips_a_fence_the_model_added_around_plain_text() {
        let revised = proofread_text(
            "helo",
            &test_config(),
            &EchoProvider("```\nhello\n```".into()),
        )
        .await
        .unwrap();
        assert_eq!(revised, "hello");
    }

    #[tokio::test]
    async fn keeps_the_fence_of_a_selected_code_block() {
        let block = "```\nlet x = 1;\n```";
        let revised = proofread_text(block, &test_config(), &EchoProvider(block.into()))
            .await
            .unwrap();
        assert_eq!(revised, block);
    }

    #[tokio::test]
    async fn fence_with_nothing_inside_is_empty_output() {
        let err = proofread_text("helo", &test_config(), &EchoProvider("```\n \n```".into()))
            .await
            .unwrap_err();
        assert!(matches!(err, ProofreadError::EmptyOutput), "got: {err}");
    }

    #[tokio::test]
    async fn blank_input_never_reaches_provider() {
        let err = proofread_text(" \n", &test_config(), &FailProvider)
            .await
            .unwrap_err();
        assert!(matches!(err, ProofreadError::NoText), "got: {err}");
    }

    #[tokio::test]
    async fn oversized_input_aborts_before_provider() {
        let err = proofread_text(&"x".repeat(101), &test_config(), &FailProvider)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ProofreadError::TooLarge(101, 100)),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn input_limit_counts_chars_not_bytes() {
        proofread_text(&"é".repeat(100), &test_config(), &EchoProvider("ok".into()))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn provider_failure_is_an_error() {
        let err = proofread_text("text", &test_config(), &FailProvider)
            .await
            .unwrap_err();
        assert!(matches!(err, ProofreadError::Provider(_)), "got: {err}");
    }

    #[tokio::test]
    async fn blank_provider_output_is_an_error() {
        let err = proofread_text("text", &test_config(), &EchoProvider("  \n".into()))
            .await
            .unwrap_err();
        assert!(matches!(err, ProofreadError::EmptyOutput), "got: {err}");
    }
}
