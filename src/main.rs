use proofd_lib::config;
use proofd_lib::proofread::proofread_text;
use proofd_lib::providers::{forget_sessions, from_config, FORGET_SESSIONS_FLAG};
use std::io::{IsTerminal, Read};
use tracing::info;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: proofd   (reads text on stdin, writes revised text to stdout)";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // stderr, not stdout: stdout carries the revised text and nothing else.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        None if !std::io::stdin().is_terminal() => proofread().await,
        Some(FORGET_SESSIONS_FLAG) => forget_opencode_sessions(&args[2..]),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("proofd {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

async fn proofread() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = config::config_path();
    let cfg = config::load_from_path(config_path.clone()).map_err(|e| {
        eprintln!("proofd: {e}");
        e
    })?;

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    info!(
        "proofread start (provider={} model={} config={} input={} chars)",
        cfg.provider,
        cfg.model,
        config_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<none>".to_string()),
        input.chars().count()
    );

    let provider = from_config(&cfg);
    let started = std::time::Instant::now();

    match proofread_text(&input, &cfg, &provider).await {
        Ok(revised) => {
            info!(
                "proofread ok ({} chars) in {}ms",
                revised.chars().count(),
                started.elapsed().as_millis()
            );
            print!("{revised}");
            Ok(())
        }
        Err(e) => {
            eprintln!("proofd: {e}");
            std::process::exit(1);
        }
    }
}

// Internal: proofd starts itself detached with this flag so deleting opencode's saved
// sessions never delays pasting the result.
fn forget_opencode_sessions(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let [bin, dir, cutoff_ms] = args else {
        return Err(
            format!("usage: proofd {FORGET_SESSIONS_FLAG} <opencode> <dir> <cutoff-ms>").into(),
        );
    };
    forget_sessions(bin, std::path::Path::new(dir), cutoff_ms.parse()?)?;
    Ok(())
}
