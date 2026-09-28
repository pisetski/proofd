use crate::config::Config;
use crate::notify::notify_failure;
use crate::platform::{Platform, SavedClip};
use crate::prompt::build_revised_only;
use crate::providers::{complete_with_timeout, Provider};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("no text found: selection empty and fallback empty; original kept")]
    NoText,
    #[error("input too large ({0} chars > max {1}); aborted")]
    TooLarge(usize, usize),
    #[error("focus changed before paste; original kept")]
    FocusChanged,
    #[error("provider error: {0}")]
    Provider(String),
    #[error("platform error: {0}")]
    Platform(String),
    #[error("busy; hotkey ignored")]
    Busy,
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

    /// Single hotkey invocation. Single-flight: returns `Busy` without work
    /// if another invocation is in flight.
    pub async fn handle_once(&self) -> Result<(), DaemonError> {
        if self.inner.busy.swap(true, Ordering::SeqCst) {
            return Err(DaemonError::Busy);
        }
        let res = self.run_flow().await;
        self.inner.busy.store(false, Ordering::SeqCst);
        res
    }

    async fn run_flow(&self) -> Result<(), DaemonError> {
        let cfg = &self.inner.config;
        let platform = &self.inner.platform;

        let pid0 = platform
            .current_app_pid()
            .map_err(|e| DaemonError::Platform(e.to_string()))?;
        let saved: SavedClip = platform
            .save_clipboard()
            .map_err(|e| DaemonError::Platform(e.to_string()))?;

        // 4-5. Selection first, then focused-text fallback.
        let mut text: Option<String> = platform
            .copy_selection()
            .map_err(|e| {
                let _ = platform.restore_clipboard(saved.clone());
                DaemonError::Platform(e.to_string())
            })?
            .filter(|s| !s.trim().is_empty());
        if text.is_none() {
            text = platform
                .copy_all_focused_text()
                .map_err(|e| {
                    let _ = platform.restore_clipboard(saved.clone());
                    DaemonError::Platform(e.to_string())
                })?
                .filter(|s| !s.trim().is_empty());
        }
        let Some(input) = text else {
            let _ = platform.restore_clipboard(saved);
            let msg = DaemonError::NoText.to_string();
            tracing::warn!("{msg}");
            notify_failure("proofd failed", "No text found; original text was kept.").await;
            return Err(DaemonError::NoText);
        };

        if input.chars().count() > cfg.max_input_chars {
            let _ = platform.restore_clipboard(saved);
            let e = DaemonError::TooLarge(input.chars().count(), cfg.max_input_chars);
            tracing::warn!("{e}");
            notify_failure(
                "proofd failed",
                "Selected text is too large; original text was kept.",
            )
            .await;
            return Err(e);
        }

        // 7-8. Provider with timeout; sanitize happens in provider.
        let (system, user) = build_revised_only(&input);
        let timeout = Duration::from_secs(cfg.timeout_secs);
        let revised = complete_with_timeout(&self.inner.provider, &system, &user, timeout)
            .await
            .map_err(|e| {
                let _ = platform.restore_clipboard(saved.clone());
                DaemonError::Provider(e.to_string())
            })?;
        if revised.trim().is_empty() {
            let _ = platform.restore_clipboard(saved);
            let msg = "provider returned empty output; original kept";
            tracing::error!("{msg}");
            notify_failure("proofd failed", "Empty result; original text was kept.").await;
            return Err(DaemonError::Provider(msg.to_string()));
        }

        // 9. Abort if focus moved while the model ran.
        let pid1 = platform.current_app_pid().map_err(|e| {
            let _ = platform.restore_clipboard(saved.clone());
            DaemonError::Platform(e.to_string())
        })?;
        if pid1 != pid0 {
            let _ = platform.restore_clipboard(saved);
            tracing::warn!("focus changed {} -> {}; paste aborted", pid0, pid1);
            notify_failure(
                "proofd failed",
                "Focus changed before paste; original text was kept.",
            )
            .await;
            return Err(DaemonError::FocusChanged);
        }

        // 10-13. Simulated paste only (never AXSetValue), then restore.
        // Single Cmd+V keeps native single Cmd+Z undo.
        if let Err(e) = platform.paste_text(&revised) {
            let _ = platform.restore_clipboard(saved);
            let msg = format!("paste failed: {e}");
            tracing::error!("{msg}");
            notify_failure("proofd failed", "Paste failed; original text was kept.").await;
            return Err(DaemonError::Platform(msg));
        }
        let _ = platform.restore_clipboard(saved);
        // 14. Silent success: only log.
        tracing::info!("polish ok ({} -> {} chars)", input.len(), revised.len());
        Ok(())
    }

    /// Map a failure to user notification + log. Success stays silent.
    pub async fn handle_and_notify(&self) {
        match self.handle_once().await {
            Ok(()) => {}
            Err(DaemonError::Busy) => {
                tracing::debug!("hotkey ignored: busy");
            }
            Err(e) => {
                // Most paths already notified inside run_flow; ensure provider/
                // platform errors that escaped still notify once.
                let s = e.to_string();
                tracing::error!("proofd failed: {s}");
                if matches!(e, DaemonError::Provider(_) | DaemonError::Platform(_)) {
                    notify_failure("proofd failed", "proofd failed; original text was kept.").await;
                }
            }
        }
    }
}

/// MVP hotkey string parser: accepts `Ctrl-Alt-P` / `Ctrl+Alt+P` /
/// `Ctrl-Option-P` styles and delegates to `global-hotkey` parsing.
/// Default maps to CONTROL | ALT + KeyP.
#[cfg(target_os = "macos")]
pub fn parse_hotkey(s: &str) -> Result<global_hotkey::hotkey::HotKey, String> {
    let normalized = s.replace('-', "+");
    normalized
        .parse()
        .map_err(|e| format!("invalid hotkey {s:?}: {e}"))
}

#[cfg(target_os = "macos")]
pub fn default_hotkey() -> global_hotkey::hotkey::HotKey {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyP)
}

/// macOS event loop: register hotkey, best-effort tray, pump main runloop.
///
/// `global-hotkey` on macOS uses Carbon `RegisterEventHotKey` targeted at the
/// application event target. The handler only fires while the main
/// `CFRunLoop` spins, so each iteration blocks briefly in
/// `CFRunLoopRunInMode` to let the OS dispatch. This MUST run on the main
/// thread (see `main.rs`: `current_thread` runtime); pumping a worker
/// thread's runloop would never receive the hotkey.
#[cfg(target_os = "macos")]
pub async fn run_macos<P, R>(daemon: Daemon<P, R>, hotkey_str: &str) -> Result<(), String>
where
    P: Platform + 'static,
    R: Provider + 'static,
{
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

    let manager = GlobalHotKeyManager::new().map_err(|e| format!("hotkey manager: {e}"))?;
    let hotkey = parse_hotkey(hotkey_str).unwrap_or_else(|e| {
        tracing::warn!("{e}; falling back to Ctrl-Alt-P");
        default_hotkey()
    });
    manager
        .register(hotkey)
        .map_err(|e| format!("register hotkey {hotkey}: {e}"))?;
    tracing::info!("hotkey registered: {hotkey}");

    // Tray indicator deferred: tray-icon needs main-thread event-loop
    // integration on macOS that would complicate the MVP. Hotkey has
    // priority; busy state is exposed via logging. See plan section 10-11.
    tracing::info!("tray deferred; using logging only");
    let _ = &daemon;

    let receiver = GlobalHotKeyEvent::receiver();

    // Ctrl-C sets a flag via a spawned task so the main-thread pump loop can
    // exit cleanly (the pump itself is a blocking 50ms wait, not awaitable).
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_task = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        shutdown_task.store(true, Ordering::SeqCst);
    });

    loop {
        // Pump the MAIN runloop so Carbon dispatches the hotkey handler,
        // which in turn sends on `receiver`. Must stay on the main thread.
        pump_runloop_once(Duration::from_millis(50));
        if shutdown.load(Ordering::SeqCst) {
            tracing::info!("shutdown");
            return Ok(());
        }
        while let Ok(event) = receiver.try_recv() {
            if event.id == hotkey.id() && event.state == HotKeyState::Pressed {
                let d = daemon.clone();
                if d.is_busy() {
                    tracing::debug!("hotkey ignored while busy");
                    continue;
                }
                tokio::spawn(async move {
                    d.handle_and_notify().await;
                });
            }
        }
        // Yield so spawned `handle_and_notify` tasks and the ctrl-c task
        // make progress on a `current_thread` runtime.
        tokio::task::yield_now().await;
    }
}

/// Block the current (must be main) thread briefly in the main runloop so
/// Carbon `EventHotKey` callbacks fire.
#[cfg(target_os = "macos")]
fn pump_runloop_once(timeout: Duration) {
    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
    // SAFETY: `kCFRunLoopDefaultMode` is a static CFString; `run_in_mode`
    // only runs the current thread's runloop for `timeout`.
    unsafe {
        CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, timeout, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{PlatformError, SavedClip};
    use crate::providers::{Provider, ProviderError};
    use std::sync::{Arc, Mutex};

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
        fn set_clipboard_text(&self, text: &str) -> Result<(), PlatformError> {
            self.state.lock().unwrap().clipboard = Some(text.to_string());
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

    #[tokio::test]
    async fn success_pastes_and_restores_clipboard() {
        let plat = MockPlatform::default();
        plat.state.lock().unwrap().clipboard = Some("orig-clip".into());
        plat.state.lock().unwrap().selection = Some("helo world".into());
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
        let err = d.handle_once().await.unwrap_err().to_string();
        assert!(
            err.contains("No text") || err.contains("no text"),
            "got: {err}"
        );
        assert_eq!(
            plat.state.lock().unwrap().clipboard.as_deref(),
            Some("keep")
        );
        assert!(plat.state.lock().unwrap().pasted.is_empty());
    }

    #[tokio::test]
    async fn focus_change_aborts_paste() {
        let plat = MockPlatform::default();
        plat.state.lock().unwrap().selection = Some("text".into());
        plat.state.lock().unwrap().clipboard = Some("clip".into());
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
        let plat = MockPlatform::default();
        plat.state.lock().unwrap().selection = Some("text".into());
        plat.state.lock().unwrap().clipboard = Some("clip".into());
        let d = Daemon::new(test_config(), plat.clone(), FailProvider);
        let err = d.handle_once().await.unwrap_err();
        assert!(matches!(err, DaemonError::Provider(_)));
        let s = plat.state.lock().unwrap();
        assert!(s.pasted.is_empty());
        assert_eq!(s.clipboard.as_deref(), Some("clip"));
    }

    #[tokio::test]
    async fn busy_guard_ignores_concurrent() {
        use std::time::Duration;
        struct Slow;
        #[async_trait::async_trait]
        impl Provider for Slow {
            async fn complete(&self, _s: &str, _u: &str) -> Result<String, ProviderError> {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok("done".into())
            }
        }
        let plat = MockPlatform::default();
        plat.state.lock().unwrap().selection = Some("text".into());
        let d = Daemon::new(test_config(), plat.clone(), Slow);
        let d2 = d.clone();
        let h = tokio::spawn(async move { d2.handle_once().await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let err = d.handle_once().await.unwrap_err();
        assert!(matches!(err, DaemonError::Busy));
        h.await.unwrap().unwrap();
    }
}
