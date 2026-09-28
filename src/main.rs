use proofd_lib::config;
use tracing::info;

// `current_thread` keeps `run_macos` on the main thread, which the event-tap
// backend requires: the main CFRunLoop must spin for tap events to dispatch.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let resolved = config::config_path();
    let cfg = config::load_from_path(None).map_err(|e| {
        eprintln!("proofd: {e}");
        e
    })?;
    info!(
        "proofd starting (provider={} model={} hotkey={} config={})",
        cfg.provider,
        cfg.model,
        cfg.hotkey,
        resolved
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<none>".to_string())
    );

    #[cfg(target_os = "macos")]
    {
        use proofd_lib::daemon::{run_macos, Daemon};
        use proofd_lib::platform_macos::MacosPlatform;
        use proofd_lib::providers::from_config;

        let platform = MacosPlatform::new();
        let provider = from_config(&cfg);
        let daemon = Daemon::new(cfg.clone(), platform, provider);
        run_macos(daemon, &cfg.hotkey).await.map_err(|e| {
            eprintln!("proofd: {e}");
            std::io::Error::other(e)
        })?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = cfg;
        eprintln!("proofd MVP is macOS-only; exiting");
        std::process::exit(2);
    }

    Ok(())
}
