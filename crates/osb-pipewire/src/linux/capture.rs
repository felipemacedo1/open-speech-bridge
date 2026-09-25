//! Audio capture stream implementation for PipeWire.
//!
//! This module provides real-time audio capture from PipeWire devices
//! using lock-free ring buffers for thread-safe audio data transfer.
//!
//! # Architecture
//!
//! ```text
//! [PipeWire Device] --> [MainLoop] --> [Stream Callback] --> [Ring Buffer] --> [Consumer]
//!                          ^                    |
//!                          |                    |
//!                     [Dedicated Thread]   [AudioRingProducer]
//!                                          (Send, lock-free)
//! ```
//!
//! # Real-time Safety
//!
//! The PipeWire audio callback runs in a real-time context. All code in the
//! `process` callback must be **real-time safe**:
//! - NO memory allocation (heap operations)
//! - NO blocking locks (mutexes, rwlocks)
//! - NO blocking I/O (disk, network)
//! - NO unbounded operations
//!
//! We achieve this by using lock-free SPSC ring buffers (`rtrb`) for audio data transfer.
//! The `AudioRingProducer` is `Send`, allowing it to be moved to the PipeWire callback thread.

use super::common::{
    build_audio_format_pod, bytes_to_f32_samples, setup_stop_timer, RT_BATCH_SIZE,
};
use super::context::PipeWireContext;
use crate::error::{PipeWireError, Result};
use osb_audio::buffer::{AudioRingBuffer, AudioRingConsumer, AudioRingProducer};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Default buffer size in frames for capture streams.
const DEFAULT_BUFFER_FRAMES: u32 = 4096;

/// Audio capture stream for recording from a PipeWire device.
///
/// # Architecture
///
/// ```text
/// [PipeWire Device] --> [CaptureStream] --> [Ring Buffer] --> [Consumer]
///                              |
///                       [ThreadLoop]
///                       (dedicated thread)
/// ```
///
/// The capture stream runs a PipeWire `ThreadLoop` in a dedicated thread.
/// Audio samples are pushed to a lock-free ring buffer using `rtrb`, which
/// provides a `Send` producer that can cross thread boundaries.
///
/// # Real-time Safety
///
/// The PipeWire process callback uses `producer.push()` which:
/// - Never blocks (lock-free SPSC buffer via `rtrb`)
/// - Never allocates (uses pre-allocated buffer)
/// - Records overflow metrics if buffer is full
///
/// # Thread Model
///
/// ```text
/// Main Thread                     PipeWire Thread (ThreadLoop)
/// -----------                     ----------------------------
/// CaptureStream::new()
///     |
///     +---> AudioRingBuffer::new() --> (producer, consumer)
///     |
/// CaptureStream::start()
///     |
///     +---> ThreadLoop::new()
///     |         |
///     |         +---> Stream::new()
///     |         |         |
///     |         |         +---> process callback (uses producer)
///     |         |                   |
///     |         |                   +---> producer.push(samples)  [REAL-TIME SAFE]
///     |         |
///     +---> threadloop.start()
///
/// Consumer (another thread)
/// -------------------------
/// consumer.pop() or consumer.pop_or_silence()
/// ```
pub struct CaptureStream {
    /// Audio format specification.
    format: AudioFormat,
    /// Target device identifier.
    device_id: String,
    /// Thread-safe running state flag (shared with callback).
    is_running: Arc<AtomicBool>,
    /// Device disconnection flag (set by PipeWire events).
    device_disconnected: Arc<AtomicBool>,
    /// Samples captured counter (atomic for cross-thread access).
    samples_captured: Arc<AtomicU64>,
    callbacks: Arc<AtomicU64>,
    /// Handle to the PipeWire thread (for cleanup).
    thread_handle: Option<std::thread::JoinHandle<()>>,
    /// Producer for manual sample injection (testing/stub mode).
    /// None when running with real PipeWire stream.
    stub_producer: Option<AudioRingProducer>,
}

impl CaptureStream {
    /// Create a new capture stream for the specified device.
    ///
    /// # Arguments
    ///
    /// * `ctx` - PipeWire context (used for initialization verification)
    /// * `device_id` - Device identifier (node ID, name, or "default")
    /// * `buffer_frames` - Size of the ring buffer in frames
    ///
    /// # Returns
    ///
    /// A tuple of `(CaptureStream, AudioRingConsumer)`. The consumer receives
    /// captured audio samples that can be processed by downstream components.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let ctx = PipeWireContext::new()?;
    /// let (mut capture, consumer) = CaptureStream::new(&ctx, "default", 4096)?;
    ///
    /// // Start capturing
    /// capture.start()?;
    ///
    /// // Read samples from consumer (in another thread/task)
    /// let mut buffer = vec![0.0f32; 1024];
    /// let read = consumer.pop(&mut buffer);
    ///
    /// // Stop when done
    /// capture.stop()?;
    /// ```
    pub fn new(
        _ctx: &PipeWireContext,
        device_id: &str,
        buffer_frames: u32,
    ) -> Result<(Self, AudioRingConsumer)> {
        let format = AudioFormat {
            sample_rate: SampleRate::PRO_48K,
            sample_format: SampleFormat::F32,
            channels: ChannelLayout::Stereo,
        };

        let buffer_frames = if buffer_frames == 0 {
            DEFAULT_BUFFER_FRAMES
        } else {
            buffer_frames
        };

        let (producer, consumer) = AudioRingBuffer::new(buffer_frames, format.channels.channels());

        info!(
            device = %device_id,
            buffer_frames = buffer_frames,
            format = %format,
            "creating capture stream"
        );

        let stream = Self {
            format,
            device_id: device_id.to_string(),
            is_running: Arc::new(AtomicBool::new(false)),
            device_disconnected: Arc::new(AtomicBool::new(false)),
            samples_captured: Arc::new(AtomicU64::new(0)),
            callbacks: Arc::new(AtomicU64::new(0)),
            thread_handle: None,
            stub_producer: Some(producer),
        };

        Ok((stream, consumer))
    }

    /// Create a new capture stream with custom audio format.
    ///
    /// # Arguments
    ///
    /// * `ctx` - PipeWire context
    /// * `device_id` - Device identifier
    /// * `buffer_frames` - Ring buffer size in frames
    /// * `format` - Custom audio format specification
    pub fn with_format(
        _ctx: &PipeWireContext,
        device_id: &str,
        buffer_frames: u32,
        format: AudioFormat,
    ) -> Result<(Self, AudioRingConsumer)> {
        let buffer_frames = if buffer_frames == 0 {
            DEFAULT_BUFFER_FRAMES
        } else {
            buffer_frames
        };

        let (producer, consumer) = AudioRingBuffer::new(buffer_frames, format.channels.channels());

        info!(
            device = %device_id,
            buffer_frames = buffer_frames,
            format = %format,
            "creating capture stream with custom format"
        );

        let stream = Self {
            format,
            device_id: device_id.to_string(),
            is_running: Arc::new(AtomicBool::new(false)),
            device_disconnected: Arc::new(AtomicBool::new(false)),
            samples_captured: Arc::new(AtomicU64::new(0)),
            callbacks: Arc::new(AtomicU64::new(0)),
            thread_handle: None,
            stub_producer: Some(producer),
        };

        Ok((stream, consumer))
    }

    /// Get the audio format of this stream.
    #[inline]
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Get the device ID.
    #[inline]
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Check if the stream is currently capturing.
    #[inline]
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    /// Check if the device has been disconnected.
    ///
    /// This can happen if the physical device is unplugged or the
    /// PipeWire node is removed.
    #[inline]
    pub fn is_device_disconnected(&self) -> bool {
        self.device_disconnected.load(Ordering::Acquire)
    }

    /// Get the total number of samples captured.
    #[inline]
    pub fn samples_captured(&self) -> u64 {
        self.samples_captured.load(Ordering::Relaxed)
    }

    /// Get the number of PipeWire process callbacks observed.
    #[inline]
    pub fn callbacks(&self) -> u64 {
        self.callbacks.load(Ordering::Relaxed)
    }

    /// Start capturing audio with real PipeWire stream.
    ///
    /// This creates a dedicated thread running a PipeWire `ThreadLoop` with
    /// a stream connected to the specified audio device. The `AudioRingProducer`
    /// is moved into the callback closure, enabling lock-free audio transfer.
    ///
    /// # PipeWire Integration
    ///
    /// 1. Creates a `ThreadLoop` in a dedicated thread
    /// 2. Creates a `Stream` with audio format negotiation
    /// 3. Connects to the target device with `AUTOCONNECT` flag
    /// 4. Registers a real-time safe process callback
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - PipeWire is not running
    /// - The specified device is not found
    /// - Failed to create or connect the stream
    pub fn start(&mut self) -> Result<()> {
        if self.is_running.load(Ordering::Acquire) {
            debug!(device = %self.device_id, "capture stream already running");
            return Ok(());
        }

        info!(device = %self.device_id, "starting capture stream");

        // Take the producer - it will be moved to the PipeWire thread
        let producer = match self.stub_producer.take() {
            Some(p) => p,
            None => {
                warn!(device = %self.device_id, "capture stream already started or producer consumed");
                return Ok(());
            }
        };

        // Clone shared state for the thread
        let is_running = Arc::clone(&self.is_running);
        let device_disconnected = Arc::clone(&self.device_disconnected);
        let samples_captured = Arc::clone(&self.samples_captured);
        let callbacks = Arc::clone(&self.callbacks);
        let device_id = self.device_id.clone();
        let format = self.format;

        // Spawn the PipeWire thread
        let handle = std::thread::Builder::new()
            .name(format!("pw-capture-{}", device_id))
            .spawn(move || {
                if let Err(e) = run_capture_loop(
                    producer,
                    &device_id,
                    format,
                    is_running,
                    device_disconnected,
                    samples_captured,
                    callbacks,
                ) {
                    error!(device = %device_id, error = %e, "capture loop failed");
                }
            })
            .map_err(|e| PipeWireError::Internal(format!("failed to spawn thread: {}", e)))?;

        self.thread_handle = Some(handle);
        self.is_running.store(true, Ordering::Release);

        info!(device = %self.device_id, "capture stream started");
        Ok(())
    }

    /// Stop capturing audio.
    ///
    /// This signals the PipeWire thread to stop and waits for it to finish.
    /// The ring buffer contents remain available for the consumer to read.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running.load(Ordering::Acquire) {
            debug!(device = %self.device_id, "capture stream already stopped");
            return Ok(());
        }

        info!(device = %self.device_id, "stopping capture stream");

        // Signal the loop to stop
        self.is_running.store(false, Ordering::Release);

        // Wait for the thread to finish
        if let Some(handle) = self.thread_handle.take() {
            // Give the thread time to notice the stop signal
            std::thread::sleep(std::time::Duration::from_millis(100));

            match handle.join() {
                Ok(()) => debug!(device = %self.device_id, "capture thread joined"),
                Err(_) => warn!(device = %self.device_id, "capture thread panicked"),
            }
        }

        let total_samples = self.samples_captured.load(Ordering::Relaxed);
        info!(
            device = %self.device_id,
            total_samples = total_samples,
            "capture stream stopped"
        );

        Ok(())
    }

    /// Push samples directly to the ring buffer (stub/test mode only).
    ///
    /// This method is only available before `start()` is called, as the
    /// producer is moved to the PipeWire thread on start.
    ///
    /// # Real-time Safety
    ///
    /// This method is real-time safe - uses lock-free ring buffer.
    ///
    /// # Returns
    ///
    /// Number of samples actually pushed (may be less than input if buffer full),
    /// or 0 if the stream has been started (producer no longer available).
    pub fn push_samples(&mut self, samples: &[f32]) -> usize {
        if let Some(ref mut producer) = self.stub_producer {
            let pushed = producer.push(samples);
            self.samples_captured
                .fetch_add(pushed as u64, Ordering::Relaxed);
            pushed
        } else {
            // Producer has been moved to PipeWire thread
            0
        }
    }

    /// Get the current buffer occupancy as a ratio (0.0 to 1.0).
    ///
    /// Note: This is only accurate in stub mode before `start()` is called.
    /// After starting, the producer is in the PipeWire thread.
    #[inline]
    pub fn buffer_occupancy(&self) -> f32 {
        if let Some(ref producer) = self.stub_producer {
            producer.occupancy().fill_ratio
        } else {
            0.0 // Can't check occupancy after producer moved
        }
    }

    /// Check if the buffer has space for the given number of frames.
    ///
    /// Note: Only accurate in stub mode before `start()` is called.
    #[inline]
    pub fn has_space_for(&self, frames: u32) -> bool {
        if let Some(ref producer) = self.stub_producer {
            producer.has_space_for(frames)
        } else {
            true // Assume space available when running
        }
    }
}

/// User data passed to PipeWire stream callbacks for capture.
///
/// This struct is owned by the PipeWire listener and provides access to
/// the ring buffer producer and shared state within the process callback.
///
/// # Real-time Safety
///
/// All fields used in the process callback must support lock-free operations:
/// - `producer`: uses `rtrb` lock-free SPSC ring buffer
/// - `samples_captured`: atomic counter, no locks
struct CaptureUserData {
    /// Lock-free ring buffer producer for audio samples.
    producer: AudioRingProducer,
    /// Atomic counter for samples captured (shared with CaptureStream).
    samples_captured: Arc<AtomicU64>,
    callbacks: Arc<AtomicU64>,
}

/// Run the PipeWire capture loop in a dedicated thread.
///
/// This function owns the `AudioRingProducer` and runs the PipeWire main loop.
/// It creates a real PipeWire stream connected to the audio device and processes
/// audio in the real-time `process` callback.
///
/// # PipeWire Integration
///
/// 1. Creates a `MainLoop` for event processing
/// 2. Creates a `Context` and connects to PipeWire daemon
/// 3. Creates a `Stream` with audio format parameters
/// 4. Registers a real-time safe `process` callback
/// 5. Connects to the target device with `AUTOCONNECT` flag
/// 6. Runs the main loop until `is_running` is set to false
///
/// # Real-time Safety
///
/// The `process` callback only uses:
/// - `producer.push()`: lock-free ring buffer write
/// - `samples_captured.fetch_add()`: atomic increment
/// - No allocations, no locks, no I/O
fn run_capture_loop(
    producer: AudioRingProducer,
    device_id: &str,
    format: AudioFormat,
    is_running: Arc<AtomicBool>,
    device_disconnected: Arc<AtomicBool>,
    samples_captured: Arc<AtomicU64>,
    callbacks: Arc<AtomicU64>,
) -> Result<()> {
    // Initialize PipeWire for this thread
    pw::init();

    let channels = format.channels.channels() as u32;
    let rate = format.sample_rate.hz();

    info!(
        device = %device_id,
        channels = channels,
        rate = rate,
        "capture loop starting with real PipeWire integration"
    );

    // Create the PipeWire main loop
    let mainloop = pw::main_loop::MainLoop::new(None).map_err(|e| {
        PipeWireError::ConnectionFailed(format!("failed to create main loop: {}", e))
    })?;

    // Create context and connect to PipeWire daemon
    let context = pw::context::Context::new(&mainloop)
        .map_err(|e| PipeWireError::ConnectionFailed(format!("failed to create context: {}", e)))?;

    let core = context.connect(None).map_err(|e| {
        PipeWireError::ConnectionFailed(format!("failed to connect to PipeWire daemon: {}", e))
    })?;

    info!(device = %device_id, "connected to PipeWire daemon");

    // Build stream properties
    let stream_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::NODE_NAME => format!("openspeechbridge-capture-{}", device_id),
    };

    // Create the capture stream
    let stream = pw::stream::Stream::new(&core, "osb-capture", stream_props).map_err(|e| {
        PipeWireError::StreamCreationFailed(format!("failed to create capture stream: {}", e))
    })?;

    // Prepare user data for callbacks
    let user_data = CaptureUserData {
        producer,
        samples_captured: Arc::clone(&samples_captured),
        callbacks,
    };

    // Clone references for callbacks
    let device_disconnected_cb = Arc::clone(&device_disconnected);
    let mainloop_weak = mainloop.downgrade();

    // Register stream listener with callbacks
    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .state_changed(move |_stream, _user_data, old, new| {
            debug!(?old, ?new, "capture stream state changed");

            match &new {
                pw::stream::StreamState::Error(err) => {
                    error!(error = %err, "capture stream error");
                    device_disconnected_cb.store(true, Ordering::Release);
                    // Quit the main loop on error
                    if let Some(ml) = mainloop_weak.upgrade() {
                        ml.quit();
                    }
                }
                pw::stream::StreamState::Unconnected => {
                    warn!("capture stream disconnected");
                    device_disconnected_cb.store(true, Ordering::Release);
                }
                pw::stream::StreamState::Streaming => {
                    info!("capture stream now streaming");
                }
                _ => {}
            }
        })
        .process(|stream, user_data| {
            // REAL-TIME SAFE CALLBACK
            // This runs in PipeWire's real-time audio thread.
            // Only lock-free operations allowed here.

            if let Some(mut buffer) = stream.dequeue_buffer() {
                user_data.callbacks.fetch_add(1, Ordering::Relaxed);
                let datas = buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    // Read chunk info first (immutable borrow)
                    let offset = data.chunk().offset() as usize;
                    let size = data.chunk().size() as usize;

                    // Now get mutable access to the data slice
                    if let Some(slice) = data.data() {
                        // Bounds check
                        if offset + size <= slice.len() {
                            let audio_bytes = &slice[offset..offset + size];
                            let _num_samples = audio_bytes.len() / 4;

                            // Process in batches using stack buffer (no allocation)
                            let mut pushed_total = 0usize;
                            let mut sample_buf = [0.0f32; RT_BATCH_SIZE];
                            let mut byte_offset = 0usize;

                            while byte_offset + 4 <= audio_bytes.len() {
                                let batch_bytes = &audio_bytes[byte_offset..];
                                let batch_samples = (batch_bytes.len() / 4).min(RT_BATCH_SIZE);

                                // Convert bytes to f32 using common helper
                                let converted = bytes_to_f32_samples(
                                    &batch_bytes[..batch_samples * 4],
                                    &mut sample_buf[..batch_samples],
                                );

                                // Push batch to ring buffer (lock-free, real-time safe)
                                let pushed = user_data.producer.push(&sample_buf[..converted]);
                                pushed_total += pushed;

                                if pushed < converted {
                                    // Buffer full, stop processing this frame
                                    break;
                                }

                                byte_offset += converted * 4;
                            }

                            // Update sample counter (atomic, real-time safe)
                            user_data
                                .samples_captured
                                .fetch_add(pushed_total as u64, Ordering::Relaxed);

                            // Note: overflow metrics are tracked internally by the producer
                            // No logging here to maintain RT safety
                        }
                    }
                }
                // Buffer is automatically queued back when dropped
            }
        })
        .register()
        .map_err(|e| {
            PipeWireError::StreamCreationFailed(format!("failed to register listener: {}", e))
        })?;

    // Build audio format parameters using common helper
    let values = build_audio_format_pod(format)?;
    let mut params = [Pod::from_bytes(&values)
        .ok_or_else(|| PipeWireError::Internal("failed to create format pod".to_string()))?];

    // Parse device_id to determine target node
    let target_id: Option<u32> = if device_id == "default" || device_id.is_empty() {
        None // Let PipeWire choose the default device
    } else {
        device_id.parse().ok() // Try to parse as node ID
    };

    // Connect the stream (Direction::Input for capture)
    stream
        .connect(
            spa::utils::Direction::Input,
            target_id,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| {
            PipeWireError::StreamCreationFailed(format!("failed to connect capture stream: {}", e))
        })?;

    info!(device = %device_id, "capture stream connected, entering main loop");

    // Set up a timer to check is_running flag periodically
    let _timer = setup_stop_timer(&mainloop, is_running, 100)?;

    // Run the main loop (blocks until quit)
    mainloop.run();

    // Cleanup
    let _ = stream.disconnect();

    let total = samples_captured.load(Ordering::Relaxed);
    info!(
        device = %device_id,
        total_samples = total,
        "capture loop exiting"
    );

    Ok(())
}

impl Drop for CaptureStream {
    fn drop(&mut self) {
        if self.is_running.load(Ordering::Acquire) {
            if let Err(e) = self.stop() {
                error!(device = %self.device_id, error = %e, "failed to stop capture stream on drop");
            }
        }
    }
}

/// Builder for CaptureStream with configuration options.
///
/// # Example
///
/// ```ignore
/// let ctx = PipeWireContext::new()?;
/// let (capture, consumer) = CaptureStreamBuilder::new("default")
///     .buffer_frames(8192)
///     .sample_rate(SampleRate::PRO_48K)
///     .channels(ChannelLayout::Stereo)
///     .build(&ctx)?;
/// ```
pub struct CaptureStreamBuilder {
    device_id: String,
    buffer_frames: u32,
    sample_rate: SampleRate,
    channels: ChannelLayout,
    sample_format: SampleFormat,
}

impl CaptureStreamBuilder {
    /// Create a new builder for the specified device.
    pub fn new(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            buffer_frames: DEFAULT_BUFFER_FRAMES,
            sample_rate: SampleRate::PRO_48K,
            channels: ChannelLayout::Stereo,
            sample_format: SampleFormat::F32,
        }
    }

    /// Set the ring buffer size in frames.
    ///
    /// Larger buffers provide more tolerance for processing jitter but
    /// increase latency. Default is 4096 frames (~85ms at 48kHz).
    pub fn buffer_frames(mut self, frames: u32) -> Self {
        self.buffer_frames = frames;
        self
    }

    /// Set the sample rate.
    ///
    /// Default is 48000 Hz (SampleRate::PRO_48K).
    pub fn sample_rate(mut self, rate: SampleRate) -> Self {
        self.sample_rate = rate;
        self
    }

    /// Set the channel layout.
    ///
    /// Default is Stereo.
    pub fn channels(mut self, channels: ChannelLayout) -> Self {
        self.channels = channels;
        self
    }

    /// Set the sample format.
    ///
    /// Default is F32 (32-bit float).
    pub fn sample_format(mut self, format: SampleFormat) -> Self {
        self.sample_format = format;
        self
    }

    /// Build the capture stream.
    pub fn build(self, ctx: &PipeWireContext) -> Result<(CaptureStream, AudioRingConsumer)> {
        let format = AudioFormat {
            sample_rate: self.sample_rate,
            sample_format: self.sample_format,
            channels: self.channels,
        };

        CaptureStream::with_format(ctx, &self.device_id, self.buffer_frames, format)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let builder = CaptureStreamBuilder::new("test-device");
        assert_eq!(builder.device_id, "test-device");
        assert_eq!(builder.buffer_frames, DEFAULT_BUFFER_FRAMES);
        assert_eq!(builder.sample_rate, SampleRate::PRO_48K);
        assert_eq!(builder.channels, ChannelLayout::Stereo);
        assert_eq!(builder.sample_format, SampleFormat::F32);
    }

    #[test]
    fn test_builder_customization() {
        let builder = CaptureStreamBuilder::new("custom-device")
            .buffer_frames(8192)
            .sample_rate(SampleRate::CD_44K)
            .channels(ChannelLayout::Mono)
            .sample_format(SampleFormat::I16);

        assert_eq!(builder.device_id, "custom-device");
        assert_eq!(builder.buffer_frames, 8192);
        assert_eq!(builder.sample_rate, SampleRate::CD_44K);
        assert_eq!(builder.channels, ChannelLayout::Mono);
        assert_eq!(builder.sample_format, SampleFormat::I16);
    }

    /// Test that requires PipeWire daemon to be running.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_capture_stream_creation() {
        let ctx = PipeWireContext::new().unwrap();
        let (stream, _consumer) = CaptureStream::new(&ctx, "default", 4096).unwrap();
        assert!(!stream.is_running());
        assert!(!stream.is_device_disconnected());
        assert_eq!(stream.samples_captured(), 0);
    }

    /// Test buffer operations in stub mode (before start).
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_capture_stream_buffer_ops_stub() {
        let ctx = PipeWireContext::new().unwrap();
        let (mut stream, mut consumer) = CaptureStream::new(&ctx, "default", 1000).unwrap();

        // Push some samples (stub mode - producer still available)
        let input = vec![0.5f32; 500];
        let pushed = stream.push_samples(&input);
        assert_eq!(pushed, 500);
        assert_eq!(stream.samples_captured(), 500);

        // Check occupancy
        let occupancy = stream.buffer_occupancy();
        assert!(occupancy > 0.2 && occupancy < 0.3); // ~25% for stereo

        // Read samples
        let mut output = vec![0.0f32; 500];
        let read = consumer.pop(&mut output);
        assert_eq!(read, 500);
        assert_eq!(input, output);
    }

    /// Test that producer moves to PipeWire thread on start.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_capture_stream_producer_moves_on_start() {
        let ctx = PipeWireContext::new().unwrap();
        let (mut stream, _consumer) = CaptureStream::new(&ctx, "default", 4096).unwrap();

        // Before start: can push samples
        assert_eq!(stream.push_samples(&[1.0; 10]), 10);

        // Start moves producer to PipeWire thread
        stream.start().unwrap();
        assert!(stream.is_running());

        // After start: push returns 0 (producer moved)
        assert_eq!(stream.push_samples(&[1.0; 10]), 0);

        stream.stop().unwrap();
    }

    /// Test start/stop lifecycle.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_capture_stream_lifecycle() {
        let ctx = PipeWireContext::new().unwrap();
        let (mut stream, _consumer) = CaptureStream::new(&ctx, "default", 4096).unwrap();

        assert!(!stream.is_running());

        stream.start().unwrap();
        assert!(stream.is_running());

        // Starting again should be idempotent
        stream.start().unwrap();
        assert!(stream.is_running());

        stream.stop().unwrap();
        assert!(!stream.is_running());

        // Stopping again should be idempotent
        stream.stop().unwrap();
        assert!(!stream.is_running());
    }
}
