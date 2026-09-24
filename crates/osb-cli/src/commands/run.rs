//! Run daemon command.

use anyhow::Result;
use osb_daemon::{Runtime, RuntimeConfig};
use tokio::signal;
use tracing::info;

pub async fn run(config_path: Option<String>, foreground: bool) -> Result<()> {
    println!("Starting OpenSpeechBridge daemon...");

    // Load configuration
    let config = if let Some(path) = config_path {
        info!(path = %path, "loading configuration");
        // TODO: Load from file
        RuntimeConfig::default()
    } else {
        RuntimeConfig::default()
    };

    if !foreground {
        println!("Note: Daemonization not yet implemented, running in foreground");
    }

    let runtime = Runtime::new(config);

    // Start the runtime
    runtime.start().await?;

    println!("Daemon running. Press Ctrl+C to stop.");

    // Wait for shutdown signal
    signal::ctrl_c().await?;

    println!();
    println!("Shutting down...");

    // Stop the runtime
    runtime.stop().await?;

    println!("Daemon stopped.");
    Ok(())
}
