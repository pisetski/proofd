use proofd_lib::proofread::proofread_text;
use proofd_lib::providers::{AnyProvider, FORGET_SESSIONS_FLAG, forget_sessions};
use proofd_lib::{config, prompt};
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;
use tokio::signal::unix::{SignalKind, signal};
use tracing::info;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: proofd   (reads text on stdin, writes revised text to stdout)";
const QUIET_ENOUGH_FOR_EDITOR_FILTERS: &str = "warn";

type Outcome = Result<(), Box<dyn std::error::Error>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    log_to_stderr_keeping_stdout_for_the_revision();

    let args: Vec<String> = std::env::args().collect();
    let outcome = match args.get(1).map(String::as_str) {
        None if !std::io::stdin().is_terminal() => proofread_stopping_cleanly_on_signal().await,
        Some(FORGET_SESSIONS_FLAG) => forget_opencode_sessions_detached_from_proofread(&args[2..]),
        Some("-h" | "--help") => {
            print_help();
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("proofd {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("proofd: {e}");
            ExitCode::FAILURE
        }
    }
}

fn log_to_stderr_keeping_stdout_for_the_revision() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(QUIET_ENOUGH_FOR_EDITOR_FILTERS));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
}

fn print_help() {
    let describe = |path: Option<PathBuf>| match path {
        Some(path) if path.exists() => path.display().to_string(),
        Some(path) => format!("{} (not found, using defaults)", path.display()),
        None => "<no config directory>".to_string(),
    };
    println!("{USAGE}");
    println!();
    println!("config: {}", describe(config::config_path()));
    println!("prompt: {}", describe(prompt::prompt_path()));
    println!("Both are optional; see https://github.com/pisetski/proofd#configure");
}

async fn proofread_stopping_cleanly_on_signal() -> Outcome {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    tokio::select! {
        outcome = proofread_stdin() => outcome,
        _ = terminate.recv() => Err("stopped by SIGTERM".into()),
        _ = interrupt.recv() => Err("stopped by SIGINT".into()),
    }
}

async fn proofread_stdin() -> Outcome {
    let config_path = config::config_path();
    let cfg = config::load(config_path.as_deref())?;

    let mut selection = String::new();
    std::io::stdin().read_to_string(&mut selection)?;

    info!(
        "proofread start (provider={} model={} config={} input={} chars)",
        cfg.provider,
        cfg.model,
        config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_string()),
        selection.chars().count()
    );

    let provider = AnyProvider::from_config(&cfg);
    let started = Instant::now();
    let revision = proofread_text(&selection, &cfg, &provider).await?;
    info!(
        "proofread ok ({} chars) in {}ms",
        revision.chars().count(),
        started.elapsed().as_millis()
    );
    print!("{revision}");
    Ok(())
}

fn forget_opencode_sessions_detached_from_proofread(args: &[String]) -> Outcome {
    let [opencode, sandbox, cutoff_ms] = args else {
        return Err(
            format!("usage: proofd {FORGET_SESSIONS_FLAG} <opencode> <dir> <cutoff-ms>").into(),
        );
    };
    forget_sessions(opencode, Path::new(sandbox), cutoff_ms.parse()?)?;
    Ok(())
}
