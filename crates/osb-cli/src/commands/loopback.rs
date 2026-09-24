//! Audio loopback test command.
//!
//! This command creates a complete audio pipeline:
//! Microphone → Ring Buffer → Virtual Microphone
//!
//! Users can select "OpenSpeechBridge" as their microphone in
//! applications like Discord, Zoom, or Meet to hear their own voice.

use anyhow::{Context, Result};
use std::io::{self, Write};
use std::time::{Duration, Instant};
use tokio::signal;
use tracing::info;

#[cfg(target_os = "linux")]
use osb_pipewire::linux::{CaptureStream, PipeWireContext, VirtualMicrophone};

/// Loopback metrics for display.
#[derive(Default)]
#[allow(dead_code)]
struct LoopbackMetrics {
    /// Total samples processed.
    samples_processed: u64,
    /// Buffer fill ratio (0.0 to 1.0).
    buffer_fill: f32,
    /// Number of buffer underruns.
    underruns: u64,
    /// Number of buffer overruns.
    overruns: u64,
    /// Estimated latency in milliseconds.
    latency_ms: f32,
    /// Duration running.
    elapsed: Duration,
}

impl LoopbackMetrics {
    fn new() -> Self {
        Self::default()
    }
}

/// Print metrics to stdout (overwrites previous line).
fn print_metrics(metrics: &LoopbackMetrics, first_print: bool) {
    // Move cursor up if not first print
    if !first_print {
        print!("\x1b[6A"); // Move up 6 lines
    }

    let buffer_bar = create_progress_bar(metrics.buffer_fill, 20);
    let elapsed_secs = metrics.elapsed.as_secs();

    println!("┌─────────────────────────────────────────────┐");
    println!(
        "│ Elapsed: {:02}:{:02}                               │",
        elapsed_secs / 60,
        elapsed_secs % 60
    );
    println!(
        "│ Samples: {:>12}                      │",
        format_number(metrics.samples_processed)
    );
    println!(
        "│ Buffer:  [{}] {:>5.1}%           │",
        buffer_bar,
        metrics.buffer_fill * 100.0
    );
    println!(
        "│ Latency: {:>6.1} ms                          │",
        metrics.latency_ms
    );
    println!("└─────────────────────────────────────────────┘");

    io::stdout().flush().ok();
}

/// Create a progress bar string.
fn create_progress_bar(ratio: f32, width: usize) -> String {
    let filled = (ratio * width as f32).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

/// Format a number with thousand separators.
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Run the loopback test.
pub async fn run(input_device: Option<String>, duration: u64, json_output: bool) -> Result<()> {
    let device = input_device.unwrap_or_else(|| "default".to_string());

    if !json_output {
        println!("╔═══════════════════════════════════════════════╗");
        println!("║     OpenSpeechBridge Audio Loopback Test      ║");
        println!("╠═══════════════════════════════════════════════╣");
        println!("║ Input device: {:<32}║", truncate_str(&device, 32));
        println!(
            "║ Duration: {:<36}║",
            if duration == 0 {
                "unlimited (Ctrl+C to stop)".to_string()
            } else {
                format!("{} seconds", duration)
            }
        );
        println!("╚═══════════════════════════════════════════════╝");
        println!();
    }

    #[cfg(target_os = "linux")]
    {
        run_linux_loopback(&device, duration, json_output).await
    }

    #[cfg(not(target_os = "linux"))]
    {
        if json_output {
            println!(
                r#"{{"error": "Loopback test is only available on Linux with PipeWire", "platform": "unsupported"}}"#
            );
        } else {
            println!("Error: Loopback test is only available on Linux with PipeWire");
            println!("This platform is not supported yet.");
            println!();
            println!("To test on Linux:");
            println!("  docker compose exec dev cargo run --bin openspeechbridge -- loopback");
        }
        Ok(())
    }
}

/// Truncate a string to max length with ellipsis.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(target_os = "linux")]
async fn run_linux_loopback(device: &str, duration: u64, json_output: bool) -> Result<()> {
    use osb_audio::buffer::AudioRingBuffer;

    info!(device = %device, "initializing loopback");

    // Initialize PipeWire context
    let ctx = PipeWireContext::new().context("Failed to initialize PipeWire")?;

    if !json_output {
        if let Some(version) = ctx.version() {
            println!("PipeWire version: {}", version);
        }
        println!();
    }

    // Buffer configuration
    // 4096 frames at 48kHz = ~85ms of buffer
    // This provides good tolerance for processing jitter
    const BUFFER_FRAMES: u32 = 4096;
    const SAMPLE_RATE: u32 = 48000;
    const CHANNELS: u8 = 2;

    // Create shared ring buffer
    // The buffer is split into producer (for capture) and consumer (for virtual mic)
    let (_producer, _consumer) = AudioRingBuffer::new(BUFFER_FRAMES, CHANNELS);

    // Create capture stream
    let (mut capture, mut capture_consumer) =
        CaptureStream::new(&ctx, device, BUFFER_FRAMES).context("Failed to create capture stream")?;

    // Create virtual microphone
    let (mut vmic, mut vmic_producer) = VirtualMicrophone::new(
        &ctx,
        "OpenSpeechBridge Virtual Microphone",
        BUFFER_FRAMES,
    )
    .context("Failed to create virtual microphone")?;

    // Start the streams
    capture.start().context("Failed to start capture")?;
    vmic.start().context("Failed to start virtual microphone")?;

    if !json_output {
        println!("✓ Capture stream started");
        println!("✓ Virtual microphone active");
        println!();
        println!("Speak into your microphone. Select 'OpenSpeechBridge Virtual");
        println!("Microphone' in your application to hear the loopback.");
        println!();
        println!("Press Ctrl+C to stop");
        println!();
    }

    let start_time = Instant::now();
    let mut metrics = LoopbackMetrics::new();
    let mut first_print = true;
    let mut transfer_buffer = vec![0.0f32; 1024];

    // Main loop
    loop {
        // Transfer audio from capture to virtual mic
        // In a real implementation with working PipeWire streams,
        // this would happen automatically via callbacks.
        // For now, we simulate the transfer for testing.
        let read = capture_consumer.pop(&mut transfer_buffer);
        if read > 0 {
            vmic_producer.push(&transfer_buffer[..read]);
            metrics.samples_processed += read as u64;
        }

        // Update metrics
        metrics.elapsed = start_time.elapsed();
        metrics.buffer_fill = capture.buffer_occupancy();
        metrics.latency_ms = (BUFFER_FRAMES as f32 / SAMPLE_RATE as f32) * 1000.0
            * metrics.buffer_fill;

        // Print metrics (every 500ms)
        if !json_output && metrics.elapsed.as_millis() % 500 < 50 {
            print_metrics(&metrics, first_print);
            first_print = false;
        }

        // Check termination conditions
        let should_stop = if duration > 0 {
            metrics.elapsed.as_secs() >= duration
        } else {
            false
        };

        if should_stop {
            break;
        }

        // Check for Ctrl+C (non-blocking)
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            _ = signal::ctrl_c() => {
                break;
            }
        }
    }

    // Cleanup
    vmic.stop().ok();
    capture.stop().ok();

    if json_output {
        println!(
            r#"{{"status": "completed", "samples_processed": {}, "duration_ms": {}, "avg_latency_ms": {:.2}}}"#,
            metrics.samples_processed,
            metrics.elapsed.as_millis(),
            metrics.latency_ms
        );
    } else {
        println!();
        println!("════════════════════════════════════════════════");
        println!("Loopback test completed");
        println!(
            "  Total samples: {}",
            format_number(metrics.samples_processed)
        );
        println!("  Duration: {:?}", metrics.elapsed);
        println!("════════════════════════════════════════════════");
    }

    Ok(())
}
