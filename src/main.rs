use proofd_lib::config;
use proofd_lib::polish::polish_text;
use proofd_lib::providers::from_config;
use std::io::{IsTerminal, Read};
use tracing::info;

const USAGE: &str = "usage: proofd polish   (reads text on stdin, writes revised text to stdout)";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // stderr, not stdout: stdout carries the revised text and nothing else.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    match std::env::args().nth(1).as_deref() {
        Some("polish") => run_polish().await,
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

async fn run_polish() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = config::config_path();
    let cfg = config::load_from_path(config_path.clone()).map_err(|e| {
        eprintln!("proofd: {e}");
        e
    })?;

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    info!(
        "polish start (provider={} model={} config={} input={} chars)",
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

    match polish_text(&input, &cfg, &provider).await {
        Ok(revised) => {
            info!(
                "polish ok ({} chars) in {}ms",
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
