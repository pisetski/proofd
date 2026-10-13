use crate::config::Config;
use crate::notify::notify_failure;
use crate::platform::{Platform, PlatformError, SavedClip};
use crate::prompt::build_revised_only;
use crate::providers::{complete_with_timeout, Provider};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

const GENERIC_FAILURE_MESSAGE: &str = "proofd failed; original text was kept.";

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("no text found: selection empty and fallback empty; original kept")]
    NoText,
    #[error("input too large ({0} chars > max {1}); aborted")]
    TooLarge(usize, usize),
    #[error("focus changed before paste; original kept")]
    FocusChanged,
    #[error("provider returned empty output; original kept")]
    EmptyOutput,
    #[error("paste failed: {0}")]
    PasteFailed(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("platform error: {0}")]
    Platform(String),
    #[error("busy; hotkey ignored")]
    Busy,
}

impl DaemonError {
    fn user_message(&self) -> Option<&'static str> {
        match self {
            Self::NoText => Some("No text found; original text was kept."),
            Self::TooLarge(..) => Some("Selected text is too large; original text was kept."),
            Self::FocusChanged => Some("Focus changed before paste; original text was kept."),
            Self::EmptyOutput => Some("Empty result; original text was kept."),
            Self::PasteFailed(_) => Some("Paste failed; original text was kept."),
            Self::Provider(_) | Self::Platform(_) => Some(GENERIC_FAILURE_MESSAGE),
            Self::Busy => None,
        }
    }
}

impl From<PlatformError> for DaemonError {
    fn from(e: PlatformError) -> Self {
        Self::Platform(e.to_string())
    }
}

pub struct Daemon<P, R> {
    inner: Arc<Inner<P, R>>,
}

struct Inner<P, R> {
    config: Config,
    platform: P,
    provider: R,
    busy: AtomicBool,
}

impl<P, R> Clone for Daemon<P, R> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct BusyGuard<'a>(&'a AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

struct ClipboardGuard<'a, P: Platform> {
    platform: &'a P,
    saved: Option<SavedClip>,
}

impl<'a, P: Platform> ClipboardGuard<'a, P> {
    fn new(platform: &'a P, saved: SavedClip) -> Self {
        Self {
            platform,
            saved: Some(saved),
        }
    }

    fn restore(&mut self) -> u128 {
        let started = Instant::now();
        if let Some(saved) = self.saved.take() {
            let _ = self.platform.restore_clipboard(saved);
        }
        started.elapsed().as_millis()
    }
}

impl<P: Platform> Drop for ClipboardGuard<'_, P> {
    fn drop(&mut self) {
        self.restore();
    }
}

struct Timings {
    started: Instant,
    pid_before_ms: u128,
    save_clipboard_ms: u128,
    copy_selection_ms: u128,
    copy_fallback_ms: u128,
    provider_ms: u128,
    pid_after_ms: u128,
    paste_ms: u128,
    restore_ms: u128,
}

impl Timings {
    fn start() -> Self {
        Self {
            started: Instant::now(),
            pid_before_ms: 0,
            save_clipboard_ms: 0,
            copy_selection_ms: 0,
            copy_fallback_ms: 0,
            provider_ms: 0,
            pid_after_ms: 0,
            paste_ms: 0,
            restore_ms: 0,
        }
    }

    fn summary(&self) -> String {
        format!(
            "total={}ms pid0={}ms save={}ms copy_sel={}ms copy_fallback={}ms provider={}ms pid1={}ms paste={}ms restore={}ms",
            self.started.elapsed().as_millis(),
            self.pid_before_ms,
            self.save_clipboard_ms,
            self.copy_selection_ms,
            self.copy_fallback_ms,
            self.provider_ms,
            self.pid_after_ms,
            self.paste_ms,
            self.restore_ms,
        )
    }
}

struct Polished {
    input_chars: usize,
    revised_chars: usize,
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, u128) {
    let started = Instant::now();
    let value = f();
    (value, started.elapsed().as_millis())
}

fn non_blank(text: Option<String>) -> Option<String> {
    text.filter(|s| !s.trim().is_empty())
}

impl<P: Platform, R: Provider> Daemon<P, R> {
    pub fn new(config: Config, platform: P, provider: R) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                platform,
                provider,
                busy: AtomicBool::new(false),
            }),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.inner.busy.load(Ordering::SeqCst)
    }

    pub async fn handle_once(&self) -> Result<(), DaemonError> {
        if self.inner.busy.swap(true, Ordering::SeqCst) {
            return Err(DaemonError::Busy);
        }
        let _busy = BusyGuard(&self.inner.busy);
        self.run_flow().await
    }

    pub async fn handle_and_notify(&self) {
        let Err(error) = self.handle_once().await else {
            return;
        };
        let Some(message) = error.user_message() else {
            tracing::debug!("hotkey ignored: busy");
            return;
        };
        tracing::error!("proofd failed: {error}");
        notify_failure("proofd failed", message).await;
    }

    async fn run_flow(&self) -> Result<(), DaemonError> {
        let platform = &self.inner.platform;
        let mut timings = Timings::start();

        let (pid_before, ms) = timed(|| platform.current_app_pid());
        timings.pid_before_ms = ms;
        let pid_before = pid_before?;

        let (saved, ms) = timed(|| platform.save_clipboard());
        timings.save_clipboard_ms = ms;
        let mut clipboard = ClipboardGuard::new(platform, saved?);

        let outcome = self.polish_in_place(pid_before, &mut timings).await;
        timings.restore_ms = clipboard.restore();

        let polished = outcome?;
        tracing::info!(
            "polish ok ({} -> {} chars) timings {}",
            polished.input_chars,
            polished.revised_chars,
            timings.summary(),
        );
        Ok(())
    }

    async fn polish_in_place(
        &self,
        pid_before: u32,
        timings: &mut Timings,
    ) -> Result<Polished, DaemonError> {
        let max_input_chars = self.inner.config.max_input_chars;
        let input = self.capture_input(timings)?;
        let input_chars = input.chars().count();
        if input_chars > max_input_chars {
            return Err(DaemonError::TooLarge(input_chars, max_input_chars));
        }

        let revised = self.revise(&input, timings).await?;
        self.ensure_focus_unchanged(pid_before, timings)?;
        self.paste(&revised, timings)?;

        Ok(Polished {
            input_chars,
            revised_chars: revised.chars().count(),
        })
    }

    fn capture_input(&self, timings: &mut Timings) -> Result<String, DaemonError> {
        let platform = &self.inner.platform;

        let (selection, ms) = timed(|| platform.copy_selection());
        timings.copy_selection_ms = ms;
        if let Some(text) = non_blank(selection?) {
            return Ok(text);
        }

        let (focused_text, ms) = timed(|| platform.copy_all_focused_text());
        timings.copy_fallback_ms = ms;
        non_blank(focused_text?).ok_or(DaemonError::NoText)
    }

    async fn revise(&self, input: &str, timings: &mut Timings) -> Result<String, DaemonError> {
        let (system, user) = build_revised_only(input);
        let timeout = Duration::from_secs(self.inner.config.timeout_secs);
        let started = Instant::now();
        let revised = complete_with_timeout(&self.inner.provider, &system, &user, timeout)
            .await
            .map_err(|e| {
                tracing::warn!(
                    "provider failed after {}ms (total {}ms): {e}",
                    started.elapsed().as_millis(),
                    timings.started.elapsed().as_millis()
                );
                DaemonError::Provider(e.to_string())
            })?;
        timings.provider_ms = started.elapsed().as_millis();
        if revised.trim().is_empty() {
            return Err(DaemonError::EmptyOutput);
        }
        Ok(revised)
    }

    fn ensure_focus_unchanged(
        &self,
        pid_before: u32,
        timings: &mut Timings,
    ) -> Result<(), DaemonError> {
        let (pid_after, ms) = timed(|| self.inner.platform.current_app_pid());
        timings.pid_after_ms = ms;
        let pid_after = pid_after?;
        if pid_after != pid_before {
            tracing::warn!("focus changed {pid_before} -> {pid_after}; paste aborted");
            return Err(DaemonError::FocusChanged);
        }
        Ok(())
    }

    fn paste(&self, revised: &str, timings: &mut Timings) -> Result<(), DaemonError> {
        let (pasted, ms) = timed(|| self.inner.platform.paste_text(revised));
        timings.paste_ms = ms;
        pasted.map_err(|e| DaemonError::PasteFailed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::PlatformError;
    use crate::providers::ProviderError;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct MockPlatform {
        state: Arc<Mutex<MockState>>,
    }

    #[derive(Default)]
    struct MockState {
        clipboard: Option<String>,
        selection: Option<String>,
        fallback: Option<String>,
        pasted: Vec<String>,
        pid: u32,
        pid_after: Option<u32>,
        pid_calls: usize,
        fail_paste: bool,
    }

    impl Platform for MockPlatform {
        fn current_app_pid(&self) -> Result<u32, PlatformError> {
            let mut s = self.state.lock().unwrap();
            s.pid_calls += 1;
            if s.pid_calls == 1 {
                return Ok(s.pid);
            }
            Ok(s.pid_after.unwrap_or(s.pid))
        }
        fn save_clipboard(&self) -> Result<SavedClip, PlatformError> {
            Ok(SavedClip {
                text: self.state.lock().unwrap().clipboard.clone(),
            })
        }
        fn restore_clipboard(&self, clip: SavedClip) -> Result<(), PlatformError> {
            self.state.lock().unwrap().clipboard = clip.text;
            Ok(())
        }
        fn copy_selection(&self) -> Result<Option<String>, PlatformError> {
            Ok(self.state.lock().unwrap().selection.clone())
        }
        fn copy_all_focused_text(&self) -> Result<Option<String>, PlatformError> {
            Ok(self.state.lock().unwrap().fallback.clone())
        }
        fn paste_text(&self, text: &str) -> Result<(), PlatformError> {
            let mut s = self.state.lock().unwrap();
            if s.fail_paste {
                return Err(PlatformError::KeySim("mock paste fail".into()));
            }
            s.pasted.push(text.to_string());
            s.clipboard = Some(text.to_string());
            Ok(())
        }
    }

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

    fn platform_with(selection: &str, clipboard: &str) -> MockPlatform {
        let plat = MockPlatform::default();
        {
            let mut s = plat.state.lock().unwrap();
            s.selection = Some(selection.into());
            s.clipboard = Some(clipboard.into());
        }
        plat
    }

    #[tokio::test]
    async fn success_pastes_and_restores_clipboard() {
        let plat = platform_with("helo world", "orig-clip");
        plat.state.lock().unwrap().pid = 111;
        let d = Daemon::new(
            test_config(),
            plat.clone(),
            EchoProvider("hello world".into()),
        );
        d.handle_once().await.unwrap();
        let s = plat.state.lock().unwrap();
        assert_eq!(s.pasted, vec!["hello world".to_string()]);
        assert_eq!(s.clipboard.as_deref(), Some("orig-clip"));
    }

    #[tokio::test]
    async fn fallback_used_when_selection_empty() {
        let plat = MockPlatform::default();
        plat.state.lock().unwrap().selection = None;
        plat.state.lock().unwrap().fallback = Some("fallback text".into());
        let d = Daemon::new(test_config(), plat.clone(), EchoProvider("ok".into()));
        d.handle_once().await.unwrap();
        assert_eq!(plat.state.lock().unwrap().pasted.len(), 1);
    }

    #[tokio::test]
    async fn empty_everywhere_errors_and_restores() {
        let plat = MockPlatform::default();
        plat.state.lock().unwrap().clipboard = Some("keep".into());
        let d = Daemon::new(test_config(), plat.clone(), EchoProvider("ok".into()));
        let err = d.handle_once().await.unwrap_err();
        assert!(matches!(err, DaemonError::NoText), "got: {err}");
        assert_eq!(
            plat.state.lock().unwrap().clipboard.as_deref(),
            Some("keep")
        );
        assert!(plat.state.lock().unwrap().pasted.is_empty());
    }

    #[tokio::test]
    async fn focus_change_aborts_paste() {
        let plat = platform_with("text", "clip");
        plat.state.lock().unwrap().pid = 1;
        plat.state.lock().unwrap().pid_after = Some(2);
        let d = Daemon::new(test_config(), plat.clone(), EchoProvider("ok".into()));
        let err = d.handle_once().await.unwrap_err();
        assert!(matches!(err, DaemonError::FocusChanged));
        let s = plat.state.lock().unwrap();
        assert!(s.pasted.is_empty());
        assert_eq!(s.clipboard.as_deref(), Some("clip"));
    }

    #[tokio::test]
    async fn provider_failure_keeps_original() {
        let plat = platform_with("text", "clip");
        let d = Daemon::new(test_config(), plat.clone(), FailProvider);
        let err = d.handle_once().await.unwrap_err();
        assert!(matches!(err, DaemonError::Provider(_)));
        let s = plat.state.lock().unwrap();
        assert!(s.pasted.is_empty());
        assert_eq!(s.clipboard.as_deref(), Some("clip"));
    }

    #[tokio::test]
    async fn oversized_input_aborts_before_provider_and_restores_clipboard() {
        let plat = platform_with(&"x".repeat(101), "clip");
        let d = Daemon::new(test_config(), plat.clone(), FailProvider);
        let err = d.handle_once().await.unwrap_err();
        assert!(matches!(err, DaemonError::TooLarge(101, 100)), "got: {err}");
        let s = plat.state.lock().unwrap();
        assert!(s.pasted.is_empty());
        assert_eq!(s.clipboard.as_deref(), Some("clip"));
    }

    #[tokio::test]
    async fn input_limit_counts_chars_not_bytes() {
        let plat = platform_with(&"é".repeat(100), "clip");
        let d = Daemon::new(test_config(), plat.clone(), EchoProvider("ok".into()));
        d.handle_once().await.unwrap();
        assert_eq!(plat.state.lock().unwrap().pasted.len(), 1);
    }

    #[tokio::test]
    async fn paste_failure_restores_clipboard() {
        let plat = platform_with("text", "clip");
        plat.state.lock().unwrap().fail_paste = true;
        let d = Daemon::new(test_config(), plat.clone(), EchoProvider("ok".into()));
        let err = d.handle_once().await.unwrap_err();
        assert!(matches!(err, DaemonError::PasteFailed(_)), "got: {err}");
        assert_eq!(
            plat.state.lock().unwrap().clipboard.as_deref(),
            Some("clip")
        );
    }

    #[tokio::test]
    async fn blank_provider_output_is_never_pasted() {
        let plat = platform_with("text", "clip");
        let d = Daemon::new(test_config(), plat.clone(), EchoProvider("  \n".into()));
        let err = d.handle_once().await.unwrap_err();
        assert!(matches!(err, DaemonError::EmptyOutput), "got: {err}");
        assert!(plat.state.lock().unwrap().pasted.is_empty());
    }

    #[tokio::test]
    async fn busy_flag_clears_after_failure() {
        let plat = platform_with("text", "clip");
        let d = Daemon::new(test_config(), plat, FailProvider);
        d.handle_once().await.unwrap_err();
        assert!(!d.is_busy());
    }

    #[tokio::test]
    async fn busy_guard_ignores_concurrent() {
        struct Slow;
        #[async_trait::async_trait]
        impl Provider for Slow {
            async fn complete(&self, _s: &str, _u: &str) -> Result<String, ProviderError> {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok("done".into())
            }
        }
        let plat = platform_with("text", "clip");
        let d = Daemon::new(test_config(), plat, Slow);
        let d2 = d.clone();
        let h = tokio::spawn(async move { d2.handle_once().await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let err = d.handle_once().await.unwrap_err();
        assert!(matches!(err, DaemonError::Busy));
        h.await.unwrap().unwrap();
    }

    #[test]
    fn every_failure_has_a_user_message_except_busy() {
        let failures = [
            DaemonError::NoText,
            DaemonError::TooLarge(2, 1),
            DaemonError::FocusChanged,
            DaemonError::EmptyOutput,
            DaemonError::PasteFailed("x".into()),
            DaemonError::Provider("x".into()),
            DaemonError::Platform("x".into()),
        ];
        for failure in failures {
            assert!(failure.user_message().is_some(), "{failure}");
        }
        assert!(DaemonError::Busy.user_message().is_none());
    }
}
