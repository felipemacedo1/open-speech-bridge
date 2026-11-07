//! Standalone persistent Unix Domain Socket worker binary for [`osb_engine_mock`].

use anyhow::{bail, Result};
use clap::Parser;
use osb_engine_mock::{MockEngine, MockEngineConfig};
use osb_protocol::framing::FrameLimits;
use std::path::PathBuf;

/// Command-line arguments for the mock engine worker process.
#[derive(Debug, Parser)]
#[command(name = "osb-engine-mock")]
#[command(about = "Persistent UDS mock engine worker for OpenSpeechBridge")]
struct Cli {
    /// Connect to a Unix Domain Socket bound by the supervisor.
    #[arg(long, env = "OSB_ENGINE_SOCKET", conflicts_with = "listen")]
    socket: Option<PathBuf>,

    /// Bind and listen on a Unix Domain Socket (standalone server mode).
    #[arg(long)]
    listen: Option<PathBuf>,

    /// Simulated processing delay per request in milliseconds.
    #[arg(long, default_value_t = 0)]
    delay_ms: u64,

    /// Gain applied to `ProcessS2s` audio frames.
    #[arg(long, default_value_t = 0.5)]
    gain: f32,

    /// Simulate a worker process crash (exit code 101) on a specific request ID.
    #[arg(long)]
    crash_on_request_id: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let crash_req = cli.crash_on_request_id;
    let engine = MockEngine::with_config(MockEngineConfig {
        simulated_delay_ms: cli.delay_ms,
        s2s_gain: cli.gain,
        crash_on_request_id: crash_req,
    });
    let limits = FrameLimits::default();

    #[cfg(unix)]
    {
        let result = if let Some(path) = cli.socket {
            osb_engine_mock::connect_and_serve_uds(&path, engine.clone(), limits).await
        } else if let Some(path) = cli.listen {
            osb_engine_mock::listen_and_serve_uds(&path, engine.clone(), limits).await
        } else {
            bail!("either --socket <PATH> (or OSB_ENGINE_SOCKET) or --listen <PATH> is required");
        };

        if let Err(osb_protocol::FramingError::UnexpectedEof) = result {
            if crash_req.is_some() || engine.config().crash_on_request_id.is_some() {
                std::process::exit(101);
            }
        }
        result?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = (engine, limits);
        bail!("Unix Domain Socket worker requires a Unix platform");
    }
}
