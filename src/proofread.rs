use crate::config::Config;
use crate::fence::strip_wrapping_fence;
use crate::prompt::ProofreadPrompt;
use crate::providers::{Provider, ProviderError};
use crate::revision_check::looks_like_revision_of;
use std::time::Duration;
use thiserror::Error;

const THINK_OPENING_TAG: &str = "<think>";
const THINK_CLOSING_TAG: &str = "</think>";

#[derive(Debug, Error)]
pub enum ProofreadError {
    #[error("no text to proofread")]
    NoText,
    #[error("input too large ({0} chars > max {1})")]
    TooLarge(usize, usize),
    #[error("provider returned empty output")]
    EmptyOutput,
    #[error("provider reply is not a revision of the selection")]
    NotARevision,
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),
}

pub async fn proofread_text<P: Provider>(
    selection: &str,
    config: &Config,
    provider: &P,
) -> Result<String, ProofreadError> {
    if selection.trim().is_empty() {
        return Err(ProofreadError::NoText);
    }
    let selection_chars = selection.chars().count();
    if selection_chars > config.max_input_chars {
        return Err(ProofreadError::TooLarge(
            selection_chars,
            config.max_input_chars,
        ));
    }

    let prompt = ProofreadPrompt::for_selection(selection);
    let reply = complete_within(config.timeout(), provider, &prompt).await?;
    let revision = strip_wrapping_fence(without_leading_think_block(&reply, selection), selection);
    if revision.is_empty() {
        return Err(ProofreadError::EmptyOutput);
    }
    if !looks_like_revision_of(selection, revision) {
        return Err(ProofreadError::NotARevision);
    }
    Ok(with_whitespace_edges_of(selection, revision))
}

async fn complete_within<P: Provider>(
    timeout: Duration,
    provider: &P,
    prompt: &ProofreadPrompt,
) -> Result<String, ProviderError> {
    tokio::time::timeout(timeout, provider.complete(&prompt.system, &prompt.user))
        .await
        .unwrap_or(Err(ProviderError::Timeout(timeout)))
}

fn without_leading_think_block<'a>(reply: &'a str, selection: &str) -> &'a str {
    let reply = reply.trim_start();
    if selection.trim_start().starts_with(THINK_OPENING_TAG) {
        return reply;
    }
    reply
        .strip_prefix(THINK_OPENING_TAG)
        .and_then(|inside| inside.split_once(THINK_CLOSING_TAG))
        .map_or(reply, |(_, after_the_block)| after_the_block)
}

fn with_whitespace_edges_of(selection: &str, revision: &str) -> String {
    let leading = &selection[..selection.len() - selection.trim_start().len()];
    let trailing = &selection[selection.trim_end().len()..];
    format!("{leading}{}{trailing}", revision.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    struct SlowProvider;
    impl Provider for SlowProvider {
        async fn complete(&self, _s: &str, _u: &str) -> Result<String, ProviderError> {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok("late".to_string())
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
    async fn drops_a_leading_think_block_before_the_corrected_text() {
        let revised = proofread_text(
            "helo",
            &test_config(),
            &EchoProvider("<think>\nThe typo is in helo.\n</think>\n\nhello".into()),
        )
        .await
        .unwrap();
        assert_eq!(revised, "hello");
    }

    #[tokio::test]
    async fn keeps_the_think_block_of_a_selection_that_starts_with_one() {
        let selection = "<think>helo</think> there";
        let revised = proofread_text(
            selection,
            &test_config(),
            &EchoProvider("<think>hello</think> there".into()),
        )
        .await
        .unwrap();
        assert_eq!(revised, "<think>hello</think> there");
    }

    #[tokio::test]
    async fn a_refusal_is_not_pasted_over_the_selection() {
        let refusal = "I'm sorry, but I can't help with that request.";
        let err = proofread_text("some text", &test_config(), &EchoProvider(refusal.into()))
            .await
            .unwrap_err();
        assert!(matches!(err, ProofreadError::NotARevision), "got: {err}");
    }

    #[tokio::test]
    async fn a_preface_before_a_short_selection_is_not_pasted() {
        let reply = "Here is the corrected text:\nhello";
        let err = proofread_text("helo", &test_config(), &EchoProvider(reply.into()))
            .await
            .unwrap_err();
        assert!(matches!(err, ProofreadError::NotARevision), "got: {err}");
    }

    #[tokio::test]
    async fn a_reply_that_lost_most_of_the_selection_is_not_pasted() {
        let selection = "The quick brown fox jumps over the lazy dog. ".repeat(2);
        let config = Config {
            max_input_chars: 1000,
            ..test_config()
        };
        let err = proofread_text(&selection, &config, &EchoProvider("The quick brown".into()))
            .await
            .unwrap_err();
        assert!(matches!(err, ProofreadError::NotARevision), "got: {err}");
    }

    #[tokio::test]
    async fn a_fence_around_the_refusal_does_not_hide_it() {
        let reply = "```\nI'm sorry, but I can't help with that request.\n```";
        let err = proofread_text("some text", &test_config(), &EchoProvider(reply.into()))
            .await
            .unwrap_err();
        assert!(matches!(err, ProofreadError::NotARevision), "got: {err}");
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
        let selection = "é".repeat(100);
        proofread_text(&selection, &test_config(), &EchoProvider(selection.clone()))
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

    #[tokio::test(start_paused = true)]
    async fn slow_provider_times_out() {
        let config = Config {
            timeout_secs: 1,
            ..test_config()
        };
        let err = proofread_text("text", &config, &SlowProvider)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ProofreadError::Provider(ProviderError::Timeout(_))),
            "got: {err}"
        );
    }
}
