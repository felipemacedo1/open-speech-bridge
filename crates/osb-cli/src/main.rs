//! OpenSpeechBridge CLI
//!
//! Command-line interface for the OpenSpeechBridge runtime.

use clap::{Parser, Subcommand};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod commands;
pub mod wav;

/// OpenSpeechBridge - Local-first real-time speech translation
#[derive(Parser)]
#[command(name = "openspeechbridge")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Output format (text, json)
    #[arg(short, long, global = true, default_value = "text")]
    format: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List audio devices
    Devices {
        /// Show only input devices
        #[arg(long)]
        input: bool,
        /// Show only output devices
        #[arg(long)]
        output: bool,
    },

    /// Check system environment and dependencies
    Doctor,

    /// Run audio loopback test (mic → virtual mic)
    Loopback {
        /// Input device ID
        #[arg(short, long)]
        input: Option<String>,
        /// Duration in seconds (0 = until Ctrl+C)
        #[arg(short, long, default_value = "10")]
        duration: u64,
    },

    /// Translate a WAV file locally through the speech-translation pipeline
    Translate(Box<commands::translate::TranslateArgs>),

    /// Start the translation daemon
    Run {
        /// Configuration file path
        #[arg(short, long)]
        config: Option<String>,
        /// Run in foreground (don't daemonize)
        #[arg(long)]
        foreground: bool,
    },

    /// Show version and build information
    Version,

    /// List available engines
    Engines,

    /// Show current status
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Emit structured logs to stderr so `--format json` stdout remains clean JSON.
    let filter = if cli.verbose {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"))
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
    };

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(filter)
        .init();

    let json_output = cli.format == "json";

    match cli.command {
        Commands::Devices { input, output } => {
            commands::devices::run(input, output, json_output).await
        }
        Commands::Doctor => commands::doctor::run(json_output).await,
        Commands::Loopback { input, duration } => {
            commands::loopback::run(input, duration, json_output, cli.verbose).await
        }
        Commands::Translate(args) => commands::translate::run(*args, json_output).await,
        Commands::Run { config, foreground } => commands::run::run(config, foreground).await,
        Commands::Version => commands::version::run(json_output),
        Commands::Engines => commands::engines::run(json_output).await,
        Commands::Status => commands::status::run(json_output).await,
    }
}
