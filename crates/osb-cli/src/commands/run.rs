//! Run daemon command (`openspeechbridge run`).
//!
//! Launches supervised UDS engine workers and connects real-time PipeWire
//! outgoing (`CaptureStream -> Pipeline -> VirtualMicrophone`) and optional
//! full-duplex incoming (`VirtualSink -> Pipeline -> PlaybackStream`) audio bridges.

use anyhow::{Context, Result};
use osb_daemon::{Pipeline, PipelineConfig, Runtime};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tracing::{info, warn};

pub async fn run(config_path: Option<String>, foreground: bool, duration_secs: u64) -> Result<()> {
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
    let mut live_bridges = if engine_count > 0 {
        start_linux_live_bridges(&runtime).await
    } else {
        LiveBridges::default()
    };

    if duration_secs > 0 {
        println!("Daemon running for {duration_secs}s (or press Ctrl+C to stop)...");
    } else {
        println!("Daemon running. Press Ctrl+C to stop.");
    }

    wait_for_shutdown_or_duration(duration_secs).await?;

    println!();
    println!("Shutting down...");

    runtime.stop().await?;

    #[cfg(target_os = "linux")]
    live_bridges.stop().await;

    println!("Daemon stopped.");
    Ok(())
}

async fn wait_for_shutdown_or_duration(duration_secs: u64) -> Result<()> {
    #[cfg(unix)]
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;

    if duration_secs > 0 {
        #[cfg(unix)]
        tokio::select! {
            res = signal::ctrl_c() => { res?; }
            _ = sigterm.recv() => {}
            _ = tokio::time::sleep(Duration::from_secs(duration_secs)) => {}
        }
        #[cfg(not(unix))]
        tokio::select! {
            res = signal::ctrl_c() => { res?; }
            _ = tokio::time::sleep(Duration::from_secs(duration_secs)) => {}
        }
    } else {
        #[cfg(unix)]
        tokio::select! {
            res = signal::ctrl_c() => { res?; }
            _ = sigterm.recv() => {}
        }
        #[cfg(not(unix))]
        signal::ctrl_c().await?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
struct OutgoingBridge {
    capture: osb_pipewire::linux::CaptureStream,
    vmic: osb_pipewire::linux::VirtualMicrophone,
    task: tokio::task::JoinHandle<()>,
}

#[cfg(target_os = "linux")]
struct IncomingBridge {
    vsink: osb_pipewire::linux::VirtualSink,
    playback: osb_pipewire::linux::PlaybackStream,
    task: tokio::task::JoinHandle<()>,
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct LiveBridges {
    outgoing: Option<OutgoingBridge>,
    incoming: Option<IncomingBridge>,
}

#[cfg(target_os = "linux")]
impl LiveBridges {
    async fn stop(&mut self) {
        if let Some(mut out) = self.outgoing.take() {
            let _ = out.capture.stop();
            let _ = out.vmic.stop();
            let _ = out.task.await;
        }
        if let Some(mut inc) = self.incoming.take() {
            let _ = inc.vsink.stop();
            let _ = inc.playback.stop();
            let _ = inc.task.await;
        }
    }
}

#[cfg(target_os = "linux")]
async fn start_linux_live_bridges(runtime: &Runtime) -> LiveBridges {
    use osb_pipewire::linux::PipeWireContext;

    let ctx = match PipeWireContext::new() {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "PipeWire unavailable; running engine supervisor without live audio streams");
            return LiveBridges::default();
        }
    };

    let cfg = runtime.config();
    let outgoing = if cfg.enable_virtual_mic {
        start_outgoing_bridge(runtime, &ctx)
    } else {
        None
    };

    let incoming = if cfg.enable_virtual_sink || cfg.config.engine.enable_incoming {
        start_incoming_bridge(runtime, &ctx)
    } else {
        None
    };

    LiveBridges { outgoing, incoming }
}

#[cfg(target_os = "linux")]
fn start_outgoing_bridge(
    runtime: &Runtime,
    ctx: &osb_pipewire::linux::PipeWireContext,
) -> Option<OutgoingBridge> {
    use osb_pipewire::linux::{CaptureStream, VirtualMicrophone};

    let cfg = runtime.config();
    let in_dev = if cfg.config.audio.input_device.is_empty() {
        "default"
    } else {
        cfg.config.audio.input_device.as_str()
    };

    let (mut capture, cap_rx) = match CaptureStream::new(ctx, in_dev, 48_000 * 5) {
        Ok(pair) => pair,
        Err(e) => {
            warn!(error = %e, "failed to create PipeWire capture stream");
            return None;
        }
    };
    let (mut vmic, out_tx) = match VirtualMicrophone::new(ctx, &cfg.virtual_mic_name, 48_000 * 15) {
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

    let eng = &cfg.config.engine;
    let mut pipeline = Pipeline::new(
        PipelineConfig {
            source_language: eng.source_language.clone(),
            target_language: eng.target_language.clone(),
            voice_id: eng.voice_id.clone(),
            chunked_tts: eng.chunked_tts,
            min_clause_words: eng.min_clause_words,
            ..PipelineConfig::default()
        }
        .with_vad_params(cfg.config.audio.vad),
    );
    let sup = Arc::clone(runtime.supervisor());
    let shutdown_rx = runtime.shutdown_receiver();

    let task = tokio::spawn(async move {
        if let Err(e) = pipeline
            .run_ring_bridge(sup, cap_rx, out_tx, shutdown_rx)
            .await
        {
            warn!(error = %e, "outgoing translation pipeline bridge exited with error");
        }
    });

    println!(
        "Outgoing live bridge active ({} -> {}, virtual mic '{}')",
        eng.source_language, eng.target_language, cfg.virtual_mic_name
    );

    Some(OutgoingBridge {
        capture,
        vmic,
        task,
    })
}

#[cfg(target_os = "linux")]
fn start_incoming_bridge(
    runtime: &Runtime,
    ctx: &osb_pipewire::linux::PipeWireContext,
) -> Option<IncomingBridge> {
    use osb_pipewire::linux::{PlaybackStream, VirtualSink};

    let cfg = runtime.config();
    let out_dev = if cfg.config.audio.output_device.is_empty() {
        "default"
    } else {
        cfg.config.audio.output_device.as_str()
    };

    let (mut vsink, sink_rx) = match VirtualSink::new(ctx, &cfg.virtual_sink_name, 48_000 * 5) {
        Ok(pair) => pair,
        Err(e) => {
            warn!(error = %e, "failed to create PipeWire virtual sink");
            return None;
        }
    };
    let (mut playback, play_tx) = match PlaybackStream::new(ctx, out_dev, 48_000 * 15) {
        Ok(pair) => pair,
        Err(e) => {
            warn!(error = %e, "failed to create PipeWire playback stream");
            return None;
        }
    };

    if let Err(e) = vsink.start() {
        warn!(error = %e, "failed to start PipeWire virtual sink");
        return None;
    }
    if let Err(e) = playback.start() {
        let _ = vsink.stop();
        warn!(error = %e, "failed to start PipeWire playback stream");
        return None;
    }

    let eng = &cfg.config.engine;
    let mut pipeline = Pipeline::new(
        PipelineConfig {
            enable_outgoing: false,
            enable_incoming: true,
            // Reverse direction for incoming remote call audio (`target_language -> source_language`)
            source_language: eng.target_language.clone(),
            target_language: eng.source_language.clone(),
            voice_id: eng
                .incoming_voice_id
                .clone()
                .or_else(|| eng.voice_id.clone()),
            chunked_tts: eng.chunked_tts,
            min_clause_words: eng.min_clause_words,
            ..PipelineConfig::default()
        }
        .with_vad_params(cfg.config.audio.vad),
    );
    let sup = Arc::clone(runtime.supervisor());
    let shutdown_rx = runtime.shutdown_receiver();

    let task = tokio::spawn(async move {
        if let Err(e) = pipeline
            .run_ring_bridge(sup, sink_rx, play_tx, shutdown_rx)
            .await
        {
            warn!(error = %e, "incoming full-duplex pipeline bridge exited with error");
        }
    });

    println!(
        "Incoming full-duplex bridge active ({} -> {}, virtual sink '{}')",
        eng.target_language, eng.source_language, cfg.virtual_sink_name
    );

    Some(IncomingBridge {
        vsink,
        playback,
        task,
    })
}
