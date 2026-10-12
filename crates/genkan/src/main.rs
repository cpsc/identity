//! `genkan`: the passkey-first OpenID Connect issuer for Kubernetes.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser as _;
use genkan::cli::{LogFormat, ServeArgs};
use genkan::output;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

fn main() -> ExitCode {
    serve(&ServeArgs::parse())
}

fn serve(args: &ServeArgs) -> ExitCode {
    init_logging(args.log_format);
    let config = match args.config() {
        Ok(config) => config,
        Err(error) => {
            output::stderr(&format!("error: {error}"));
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(args.worker_threads.max(1))
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            output::stderr(&format!("error: starting the async runtime: {error}"));
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(genkan::serve::serve(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "exiting");
            ExitCode::FAILURE
        }
    }
}

/// Logs go to stderr. `GENKAN_LOG` takes comma-separated `target=level` directives, for example
/// `info,genkan=debug`; an invalid value falls back to `info` with a warning.
fn init_logging(format: LogFormat) {
    let directives = std::env::var("GENKAN_LOG").unwrap_or_default();
    let (filter, invalid) = match directives.parse::<Targets>() {
        Ok(filter) if !directives.is_empty() => (filter, None),
        Ok(_) => (Targets::new().with_default(LevelFilter::INFO), None),
        Err(error) => (Targets::new().with_default(LevelFilter::INFO), Some(error)),
    };
    let builder = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false);
    match format {
        LogFormat::Json => builder.json().finish().with(filter).init(),
        LogFormat::Text => builder.finish().with(filter).init(),
    }
    if let Some(error) = invalid {
        tracing::warn!(%error, "ignoring invalid GENKAN_LOG");
    }
}
