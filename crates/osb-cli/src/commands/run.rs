//! Run daemon command.

use anyhow::{Context, Result};
use osb_daemon::{Pipeline, PipelineConfig, Runtime};
use std::path::Path;
use std::sync::Arc;
use tokio::signal;
use tracing::{info, warn};

pub async fn run(config_path: Option<String>, foreground: bool) -> Result<()> {
    println!("Starting OpenSpeechBridge daemon...");

    let runtime = if let Some(ref path) = config_path {
        info!(path = %path, "loading daemon configuration");
        Runtime::from_config_file(Path::new(path))
            .with_context(|| format!("failed to load configuration from {path}"))?
    } else {
        Runtime::with_defaults()
    };

    if !foreground {
        println!("Note: Daemonization not yet implemented, running in foreground");
    }

    runtime.start().await?;

    let engine_count = runtime.supervisor().lock().await.list_engines().len();
    println!("Supervised engines ready: {engine_count}");

    #[cfg(target_os = "linux")]
    let mut live_bridge = if engine_count > 0 {
        start_linux_live_bridge(&runtime).await
    } else {
        None
    };

    println!("Daemon running. Press Ctrl+C to stop.");

    signal::ctrl_c().await?;

    println!();
    println!("Shutting down...");

    runtime.stop().await?;

    #[cfg(target_os = "linux")]
    if let Some(ref mut bridge) = live_bridge {
        bridge.stop().await;
    }

    println!("Daemon stopped.");
    Ok(())
}

#[cfg(target_os = "linux")]
struct LiveBridgeHandles {
    capture: osb_pipewire::linux::CaptureStream,
    vmic: osb_pipewire::linux::VirtualMicrophone,
    task: tokio::task::JoinHandle<()>,
}

#[cfg(target_os = "linux")]
impl LiveBridgeHandles {
    async fn stop(&mut self) {
        let _ = self.capture.stop();
        let _ = self.vmic.stop();
        let _ = (&mut self.task).await;
    }
}

#[cfg(target_os = "linux")]
async fn start_linux_live_bridge(runtime: &Runtime) -> Option<LiveBridgeHandles> {
    use osb_pipewire::linux::{CaptureStream, PipeWireContext, VirtualMicrophone};

    let ctx = match PipeWireContext::new() {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "PipeWire unavailable; running engine supervisor without live audio streams");
            return None;
        }
    };

    let cfg = runtime.config();
    let in_dev = if cfg.config.audio.input_device.is_empty() {
        "default"
    } else {
        cfg.config.audio.input_device.as_str()
    };

    // Bounded pre-allocated SPSC rings: ~5s capture, ~15s virtual mic output for TTS utterances.
    let (mut capture, cap_rx) = match CaptureStream::new(&ctx, in_dev, 48_000 * 5) {
        Ok(pair) => pair,
        Err(e) => {
            warn!(error = %e, "failed to create PipeWire capture stream");
            return None;
        }
    };
    let (mut vmic, out_tx) = match VirtualMicrophone::new(&ctx, &cfg.virtual_mic_name, 48_000 * 15)
    {
        Ok(pair) => pair,
        Err(e) => {
            warn!(error = %e, "failed to create PipeWire virtual microphone");
            return None;
        }
    };

    if let Err(e) = capture.start() {
        warn!(error = %e, "failed to start PipeWire capture stream");
        return None;
    }
    if let Err(e) = vmic.start() {
        let _ = capture.stop();
        warn!(error = %e, "failed to start PipeWire virtual microphone");
        return None;
    }

    let mut pipeline = Pipeline::new(PipelineConfig {
        source_language: cfg.config.engine.source_language.clone(),
        target_language: cfg.config.engine.target_language.clone(),
        ..PipelineConfig::default()
    });
    let sup = Arc::clone(runtime.supervisor());
    let shutdown_rx = runtime.shutdown_receiver();

    let task = tokio::spawn(async move {
        if let Err(e) = pipeline
            .run_ring_bridge(sup, cap_rx, out_tx, shutdown_rx)
            .await
        {
            warn!(error = %e, "live translation pipeline bridge exited with error");
        }
    });

    println!(
        "Live translation bridge active ({} -> {}, virtual mic '{}')",
        cfg.config.engine.source_language, cfg.config.engine.target_language, cfg.virtual_mic_name
    );

    Some(LiveBridgeHandles {
        capture,
        vmic,
        task,
    })
}
