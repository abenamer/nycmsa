//! The `nycmsa` binary. It parses the command line, hands each subcommand to
//! the composition root, and sets the exit status (REAS-RUST-005).

use std::future::Future;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use nycmsa::adapters::cli::{Cli, Command};
use nycmsa::composition::{self, AppError};

/// Blocking simulation tasks still running at shutdown get this long before
/// the process exits (REAS-RUST-007).
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(30);

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let outcome = match cli.command {
        Command::Sim(args) => composition::run_sim(&args).map(|text| print!("{text}")),
        Command::Scan(args) => composition::run_scan(&args).map(|text| print!("{text}")),
        Command::Fetch(args) => {
            with_runtime(|| composition::run_fetch(&args)).map(|text| eprint!("{text}"))
        }
        Command::FetchScores(args) => {
            with_runtime(|| composition::run_fetch_scores(&args)).map(|text| eprint!("{text}"))
        }
        Command::FetchProfiles(args) => {
            with_runtime(|| composition::run_fetch_profiles(&args)).map(|text| eprint!("{text}"))
        }
        Command::ReclassifyProfiles(args) => {
            with_runtime(|| composition::run_reclassify_profiles(&args))
                .map(|text| eprint!("{text}"))
        }
        Command::Import(args) => {
            with_runtime(|| composition::run_import(&args)).map(|text| eprint!("{text}"))
        }
        Command::Bootstrap(args) => {
            with_runtime(|| composition::run_bootstrap(&args)).map(|text| eprint!("{text}"))
        }
        Command::Serve(args) => with_runtime(|| composition::run_serve(&args)),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            let mut source = std::error::Error::source(&error);
            while let Some(inner) = source {
                eprintln!("  caused by: {inner}");
                source = inner.source();
            }
            ExitCode::FAILURE
        }
    }
}

/// Runs an async entry point on a multi-threaded runtime, then shuts the
/// runtime down with a deadline so unfinished blocking work cannot hold the
/// process open.
fn with_runtime<F, T>(entry: impl FnOnce() -> F) -> Result<T, AppError>
where
    F: Future<Output = Result<T, AppError>>,
{
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(AppError::Runtime)?;
    let result = runtime.block_on(entry());
    runtime.shutdown_timeout(SHUTDOWN_DEADLINE);
    result
}
