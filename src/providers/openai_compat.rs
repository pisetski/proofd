use super::{Provider, ProviderError};
use std::time::Instant;

const LOW_TEMPERATURE_FOR_MINIMAL_EDITS: f64 = 0.2;

#[derive(Debug, Clone)]
pub struct OpenAICompat {
    pub model: String,
    pub base_url: String,
    pub client: reqwest::Client,
}

impl Provider for OpenAICompat {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        let total = Instant::now();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": LOW_TEMPERATURE_FOR_MINIMAL_EDITS,
        });
        let send = Instant::now();
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Transport(format!("openai-compat request: {e}")))?;
        let send_ms = send.elapsed().as_millis();
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Failed(format!(
                "openai-compat {status}: {}",
                text.trim()
            )));
        }
        let decode = Instant::now();
        let value: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Transport(format!("openai-compat decode: {e}")))?;
        tracing::debug!(
            "openai-compat timings send={send_ms}ms decode={}ms total={}ms model={}",
            decode.elapsed().as_millis(),
            total.elapsed().as_millis(),
            self.model,
        );
        if value
            .pointer("/choices/0/finish_reason")
            .and_then(|r| r.as_str())
            == Some("length")
        {
            return Err(ProviderError::Failed(
                "openai-compat: the reply was cut off at the server's token limit".to_string(),
            ));
        }
        let content = value
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .unwrap_or_default();
        Ok(content.to_string())
    }
}
