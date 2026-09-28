//! Error notification only. No sound or beep. Success is silent.

/// Show a macOS notification on failure paths. Best-effort; never fails the daemon.
pub async fn notify_failure(summary: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        let summary = summary.to_string();
        let body = body.to_string();
        let _ = tokio::task::spawn_blocking(move || {
            let script = format!(
                "display notification {body} with title {title}",
                body = applescript_string(&body),
                title = applescript_string(&summary),
            );
            let _ = std::process::Command::new("osascript")
                .args(["-e", &script])
                .output();
        })
        .await;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (summary, body);
        tracing::warn!("notify (non-macOS stub): {summary}: {body}");
    }
}

#[cfg(target_os = "macos")]
fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
