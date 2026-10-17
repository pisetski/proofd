use proofd_lib::config;
use proofd_lib::daemon::polish_text;
use proofd_lib::providers::from_config;
use std::io::Read;
use tracing::info;

// current_thread keeps the macOS event loop on the main thread, where the main CFRunLoop spins.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 && args[1] == "polish" {
        return run_polish().await;
    }

    let config_path = config::config_path();
    let cfg = config::load_from_path(config_path.clone()).map_err(|e| {
        eprintln!("proofd: {e}");
        e
    })?;
    info!(
        "proofd starting (provider={} model={} hotkey={} config={})",
        cfg.provider,
        cfg.model,
        cfg.hotkey,
        config_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<none>".to_string())
    );

    #[cfg(target_os = "macos")]
    {
        use proofd_lib::daemon::Daemon;
        use proofd_lib::macos_loop::run_macos;
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

async fn run_polish() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = config::config_path();
    let cfg = config::load_from_path(config_path.clone()).map_err(|e| {
        eprintln!("proofd: {e}");
        e
    })?;

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    let provider = from_config(&cfg);

    match polish_text(&input, &cfg, &provider).await {
        Ok(revised) => {
            print!("{revised}");
            Ok(())
        }
        Err(e) => {
            eprintln!("proofd: {e}");
            std::process::exit(1);
        }
    }
}
