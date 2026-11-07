//! Virtual audio device implementation for PipeWire.
//!
//! Virtual devices allow OpenSpeechBridge to inject audio into applications
//! (virtual microphone) or capture audio from applications (virtual sink).
//!
//! # Architecture
//!
//! ```text
//! VirtualMicrophone (Audio Source):
//! [Producer] --> [Ring Buffer] --> [VirtualMicrophone] --> [PipeWire] --> [Apps]
//!                                         |
//!                                   [Dedicated Thread]
//!                                   (owns Consumer)
//!
//! VirtualSink (Audio Sink):
//! [Apps] --> [PipeWire] --> [VirtualSink] --> [Ring Buffer] --> [Consumer]
//!                                |
//!                          [Dedicated Thread]
//!                          (owns Producer)
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
//! Both `AudioRingProducer` and `AudioRingConsumer` are `Send`, allowing them to be
//! moved to PipeWire callback threads.

use super::common::{
    build_audio_format_pod, bytes_to_f32_samples, f32_samples_to_bytes, setup_stop_timer,
    SignalMetrics, SignalMetricsSnapshot, RT_BATCH_SIZE,
};
use super::context::PipeWireContext;
use crate::error::{PipeWireError, Result};
use osb_audio::buffer::{AudioRingBuffer, AudioRingConsumer, AudioRingProducer};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Default buffer size in frames for virtual devices.
const DEFAULT_BUFFER_FRAMES: u32 = 4096;

/// Callback and demand counters for a virtual microphone.
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtualMicMetricsSnapshot {
    pub callbacks: u64,
    pub frames_requested: u64,
    pub frames_consumed: u64,
    /// C: signal popped from the virtual microphone ring buffer.
    pub ring_output: SignalMetricsSnapshot,
    /// D: signal immediately before conversion into the SPA output buffer.
    pub output_before_copy: SignalMetricsSnapshot,
    /// D: signal decoded back from the SPA output buffer after the copy.
    pub output_after_copy: SignalMetricsSnapshot,
    pub buffer: VirtualMicBufferSnapshot,
}

/// Most recently observed SPA buffer geometry for the virtual microphone.
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtualMicBufferSnapshot {
    pub initial_offset: u64,
    pub initial_size: u64,
    pub maxsize: u64,
    pub requested_frames: u64,
    pub published_offset: u64,
    pub published_size: u64,
    pub published_stride: u64,
}

#[derive(Debug, Default)]
struct VirtualMicMetrics {
    signal_metrics_enabled: AtomicBool,
    callbacks: AtomicU64,
    frames_requested: AtomicU64,
    frames_consumed: AtomicU64,
    ring_output: SignalMetrics,
    output_before_copy: SignalMetrics,
    output_after_copy: SignalMetrics,
    initial_offset: AtomicU64,
    initial_size: AtomicU64,
    maxsize: AtomicU64,
    requested_frames: AtomicU64,
    published_offset: AtomicU64,
    published_size: AtomicU64,
    published_stride: AtomicU64,
}

impl VirtualMicMetrics {
    #[inline]
    fn signal_metrics_enabled(&self) -> bool {
        self.signal_metrics_enabled.load(Ordering::Relaxed)
    }

    fn snapshot(&self) -> VirtualMicMetricsSnapshot {
        VirtualMicMetricsSnapshot {
            callbacks: self.callbacks.load(Ordering::Relaxed),
            frames_requested: self.frames_requested.load(Ordering::Relaxed),
            frames_consumed: self.frames_consumed.load(Ordering::Relaxed),
            ring_output: self.ring_output.snapshot(),
            output_before_copy: self.output_before_copy.snapshot(),
            output_after_copy: self.output_after_copy.snapshot(),
            buffer: VirtualMicBufferSnapshot {
                initial_offset: self.initial_offset.load(Ordering::Relaxed),
                initial_size: self.initial_size.load(Ordering::Relaxed),
                maxsize: self.maxsize.load(Ordering::Relaxed),
                requested_frames: self.requested_frames.load(Ordering::Relaxed),
                published_offset: self.published_offset.load(Ordering::Relaxed),
                published_size: self.published_size.load(Ordering::Relaxed),
                published_stride: self.published_stride.load(Ordering::Relaxed),
            },
        }
    }
}

/// A virtual microphone that appears as an audio input device.
///
/// Applications like Discord, Zoom, or Google Meet can select this
/// virtual device as their microphone input. Audio written to this
/// device will be "heard" by those applications.
///
/// # Architecture
///
/// ```text
/// [Producer] --> [Ring Buffer] --> [VirtualMicrophone Thread] --> [PipeWire] --> [Apps]
///     ^                                      |
///     |                                [Dedicated Thread]
/// (returned to caller)                 (owns Consumer)
/// ```
///
/// - The producer (returned by `new()`) receives translated audio from caller
/// - The VirtualMicrophone runs a thread that reads from the ring buffer
/// - PipeWire callback sends audio to applications
///
/// # Real-time Safety
///
/// The `process` callback uses `pop_or_silence()` which:
/// - Never blocks (lock-free SPSC buffer via `rtrb`)
/// - Never allocates (fills existing buffer in-place)
/// - Always returns valid audio (silence on underrun)
///
/// # Thread Model
///
/// The `AudioRingConsumer` is `Send` (via `rtrb`), allowing it to be moved
/// to the PipeWire thread where it will read audio data lock-free.
pub struct VirtualMicrophone {
    /// Display name for the virtual microphone.
    name: String,
    /// Audio format specification.
    format: AudioFormat,
    /// Thread-safe running state flag.
    is_running: Arc<AtomicBool>,
    /// PipeWire node ID when registered (set after stream creation).
    node_id: Arc<AtomicU32>,
    metrics: Arc<VirtualMicMetrics>,
    /// Thread handle for the PipeWire main loop.
    thread_handle: Option<std::thread::JoinHandle<()>>,
    /// Consumer for stub mode (before start). None after start().
    stub_consumer: Option<AudioRingConsumer>,
}

impl VirtualMicrophone {
    /// Create a new virtual microphone.
    ///
    /// # Arguments
    ///
    /// * `ctx` - PipeWire context (used for initialization verification)
    /// * `name` - Name that will appear in application device lists
    /// * `buffer_frames` - Ring buffer size in frames (default: 4096)
    ///
    /// # Returns
    ///
    /// A tuple of `(VirtualMicrophone, AudioRingProducer)`. Write translated
    /// audio to the producer, and applications will receive it from the
    /// virtual microphone.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let ctx = PipeWireContext::new()?;
    /// let (mut vmic, mut producer) = VirtualMicrophone::new(&ctx, "My Virtual Mic", 4096)?;
    ///
    /// // Start the virtual microphone
    /// vmic.start()?;
    ///
    /// // Write audio to the producer (from any thread - producer is Send)
    /// producer.push(&audio_samples);
    ///
    /// // Stop when done
    /// vmic.stop()?;
    /// ```
    pub fn new(
        _ctx: &PipeWireContext,
        name: &str,
        buffer_frames: u32,
    ) -> Result<(Self, AudioRingProducer)> {
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
            name = %name,
            buffer_frames = buffer_frames,
            format = %format,
            "creating virtual microphone"
        );

        let vmic = Self {
            name: name.to_string(),
            format,
            is_running: Arc::new(AtomicBool::new(false)),
            node_id: Arc::new(AtomicU32::new(0)),
            metrics: Arc::new(VirtualMicMetrics::default()),
            thread_handle: None,
            stub_consumer: Some(consumer),
        };

        Ok((vmic, producer))
    }

    /// Get the name of this virtual microphone.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the audio format.
    #[inline]
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Get the PipeWire node ID (if registered).
    ///
    /// Returns `Some(id)` after `start()` successfully registers the node,
    /// or `None` if not yet started or if registration failed.
    #[inline]
    pub fn node_id(&self) -> Option<u32> {
        let id = self.node_id.load(Ordering::Acquire);
        if id == 0 {
            None
        } else {
            Some(id)
        }
    }

    /// Check if the virtual microphone is active.
    #[inline]
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    /// Return callback demand observed by the virtual microphone.
    pub fn metrics(&self) -> VirtualMicMetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Enable or disable aggregate signal diagnostics in the audio callback.
    ///
    /// Disabled by default so normal operation does not scan every sample for
    /// RMS and peak values. Enable this before `start()` for verbose output.
    pub fn set_signal_metrics_enabled(&self, enabled: bool) {
        self.metrics
            .signal_metrics_enabled
            .store(enabled, Ordering::Relaxed);
    }

    /// Start the virtual microphone.
    ///
    /// This creates a PipeWire node with `media.class = "Audio/Source"` and
    /// `node.virtual = true`
    /// and registers it with the PipeWire graph. The device will appear in
    /// application device lists (Discord, Zoom, etc.) as an available microphone.
    ///
    /// The `AudioRingConsumer` is moved to a dedicated thread where it will
    /// be used by the PipeWire process callback.
    ///
    /// # PipeWire Node Properties
    ///
    /// - `media.class = "Audio/Source"` - Identifies as an audio capture device
    /// - `node.virtual = true` - Marks the source as virtual
    /// - `node.name = "openspeechbridge-virtual-mic"` - Internal identifier
    /// - `node.description = "<user-provided name>"` - Display name in apps
    /// - `audio.rate = 48000` - Sample rate
    /// - `audio.channels = 2` - Stereo
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - PipeWire is not running
    /// - Failed to create the stream
    /// - Failed to register the node
    pub fn start(&mut self) -> Result<()> {
        if self.is_running.load(Ordering::Acquire) {
            debug!(name = %self.name, "virtual microphone already running");
            return Ok(());
        }

        info!(name = %self.name, "starting virtual microphone");

        // Take the consumer - it will be moved to the PipeWire thread
        let consumer = match self.stub_consumer.take() {
            Some(c) => c,
            None => {
                warn!(name = %self.name, "virtual microphone already started or consumer consumed");
                return Ok(());
            }
        };

        // Clone shared state for the thread
        let is_running = Arc::clone(&self.is_running);
        let node_id = Arc::clone(&self.node_id);
        let metrics = Arc::clone(&self.metrics);
        let name = self.name.clone();
        let format = self.format;

        // Spawn the PipeWire thread
        let handle = std::thread::Builder::new()
            .name(format!("pw-vmic-{}", name))
            .spawn(move || {
                if let Err(e) =
                    run_virtual_mic_loop(consumer, &name, format, is_running, node_id, metrics)
                {
                    error!(name = %name, error = %e, "virtual mic loop failed");
                }
            })
            .map_err(|e| PipeWireError::Internal(format!("failed to spawn thread: {}", e)))?;

        self.thread_handle = Some(handle);
        self.is_running.store(true, Ordering::Release);

        info!(name = %self.name, "virtual microphone started");
        Ok(())
    }

    /// Stop the virtual microphone.
    ///
    /// This removes the PipeWire node from the graph. Applications will no
    /// longer see this device in their microphone lists.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running.load(Ordering::Acquire) {
            debug!(name = %self.name, "virtual microphone already stopped");
            return Ok(());
        }

        info!(name = %self.name, "stopping virtual microphone");

        // Signal the loop to stop
        self.is_running.store(false, Ordering::Release);

        // Wait for the thread to finish
        if let Some(handle) = self.thread_handle.take() {
            // Give the thread a moment to notice the stop signal
            std::thread::sleep(std::time::Duration::from_millis(100));

            match handle.join() {
                Ok(()) => debug!(name = %self.name, "virtual mic thread joined"),
                Err(_) => warn!(name = %self.name, "virtual mic thread panicked"),
            }
        }

        self.node_id.store(0, Ordering::Release);
        info!(name = %self.name, "virtual microphone stopped");
        Ok(())
    }

    /// Read samples to provide to applications (stub mode only).
    ///
    /// This method is only available before `start()` is called, as the
    /// consumer is moved to the PipeWire thread on start.
    ///
    /// # Real-time Safety
    ///
    /// This method is real-time safe:
    /// - Uses lock-free ring buffer
    /// - Never allocates memory
    /// - Fills with silence on underrun (no glitches)
    ///
    /// # Arguments
    ///
    /// * `output` - Buffer to fill with audio samples (interleaved stereo F32)
    ///
    /// # Returns
    ///
    /// Number of samples actually read from the buffer (may be less than
    /// `output.len()` if buffer was partially empty; remainder is silence),
    /// or 0 if the stream has been started (consumer no longer available).
    pub fn read_samples(&mut self, output: &mut [f32]) -> usize {
        if let Some(ref mut consumer) = self.stub_consumer {
            consumer.pop_or_silence(output)
        } else {
            // Consumer has been moved to PipeWire thread, fill with silence
            output.fill(0.0);
            0
        }
    }

    /// Get current buffer occupancy as a ratio (0.0 to 1.0).
    ///
    /// Note: Only accurate in stub mode before `start()` is called.
    #[inline]
    pub fn buffer_occupancy(&self) -> f32 {
        if let Some(ref consumer) = self.stub_consumer {
            consumer.occupancy().fill_ratio
        } else {
            0.0 // Can't check occupancy after consumer moved
        }
    }
}

/// User data passed to PipeWire stream callbacks for virtual microphone.
///
/// This struct is owned by the PipeWire listener and provides access to
/// the ring buffer consumer within the process callback.
///
/// # Real-time Safety
///
/// All fields used in the process callback must support lock-free operations:
/// - `consumer`: uses `rtrb` lock-free SPSC ring buffer
struct VirtualMicUserData {
    /// Lock-free ring buffer consumer for audio samples.
    consumer: AudioRingConsumer,
    metrics: Arc<VirtualMicMetrics>,
}

// A source is a PipeWire output stream: the client fills dequeued buffers and
// queues them for downstream capture clients.
const VIRTUAL_MIC_STREAM_DIRECTION: spa::utils::Direction = spa::utils::Direction::Output;

#[derive(Debug, Clone, Copy, Default)]
struct OutputRenderStats {
    frames_requested: u64,
    frames_consumed: u64,
    bytes_written: usize,
}

/// Render interleaved F32LE audio into an output buffer owned by PipeWire.
///
/// The output buffer capacity (`spa_data.maxsize`) is the writable extent for
/// an output stream. The incoming chunk offset/size describes valid input data
/// and must not be used to size an output write.
fn render_virtual_mic_output(
    consumer: &mut AudioRingConsumer,
    output: &mut [u8],
    channels: usize,
    metrics: &VirtualMicMetrics,
) -> OutputRenderStats {
    if channels == 0 {
        return OutputRenderStats::default();
    }

    let frame_bytes = channels * std::mem::size_of::<f32>();
    let writable_bytes = (output.len() / frame_bytes) * frame_bytes;
    let mut sample_buf = [0.0f32; RT_BATCH_SIZE];
    let mut byte_offset = 0usize;
    let mut frames_consumed = 0u64;
    let collect_signal_metrics = metrics.signal_metrics_enabled();

    while byte_offset < writable_bytes {
        let remaining_samples = (writable_bytes - byte_offset) / 4;
        let batch_capacity = remaining_samples.min(RT_BATCH_SIZE);
        let batch_samples = (batch_capacity / channels) * channels;
        if batch_samples == 0 {
            break;
        }

        let read = consumer.pop_or_silence(&mut sample_buf[..batch_samples]);
        if collect_signal_metrics {
            metrics.ring_output.record(&sample_buf[..read]);
            metrics
                .output_before_copy
                .record(&sample_buf[..batch_samples]);
        }

        let written = f32_samples_to_bytes(
            &sample_buf[..batch_samples],
            &mut output[byte_offset..writable_bytes],
        );
        if collect_signal_metrics {
            metrics
                .output_after_copy
                .record_f32le_bytes(&output[byte_offset..byte_offset + written]);
        }

        frames_consumed += (read / channels) as u64;
        byte_offset += written;
    }

    OutputRenderStats {
        frames_requested: (byte_offset / frame_bytes) as u64,
        frames_consumed,
        bytes_written: byte_offset,
    }
}

#[inline]
fn requested_output_bytes(maxsize: usize, requested_frames: usize, frame_stride: usize) -> usize {
    let requested_bytes = requested_frames.saturating_mul(frame_stride);
    if requested_bytes == 0 {
        maxsize
    } else {
        maxsize.min(requested_bytes)
    }
}

/// Run the PipeWire virtual microphone loop in a dedicated thread.
///
/// This function owns the `AudioRingConsumer` and provides audio to PipeWire.
/// It creates a real PipeWire stream with `media.class = "Audio/Source"` and
/// `node.virtual = true`
/// that appears as a microphone in applications.
///
/// # PipeWire Integration
///
/// 1. Creates a `MainLoop` for event processing
/// 2. Creates a `Context` and connects to PipeWire daemon
/// 3. Creates a `Stream` with virtual audio source properties
/// 4. Registers a real-time safe `process` callback
/// 5. Connects with `Direction::Output` so the client publishes source audio
/// 6. Runs the main loop until `is_running` is set to false
///
/// # Real-time Safety
///
/// The `process` callback only uses:
/// - `consumer.pop_or_silence()`: lock-free ring buffer read
/// - No allocations, no locks, no I/O
fn run_virtual_mic_loop(
    consumer: AudioRingConsumer,
    name: &str,
    format: AudioFormat,
    is_running: Arc<AtomicBool>,
    node_id: Arc<AtomicU32>,
    metrics: Arc<VirtualMicMetrics>,
) -> Result<()> {
    // Initialize PipeWire for this thread
    pw::init();

    let channels = format.channels.channels() as u32;
    let rate = format.sample_rate.hz();

    info!(
        name = %name,
        channels = channels,
        rate = rate,
        "virtual mic loop starting with real PipeWire integration"
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

    info!(name = %name, "connected to PipeWire daemon for virtual mic");

    // Build stream properties for virtual audio source
    // Audio/Source plus node.virtual makes this appear as a virtual microphone.
    let stream_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        // The stream produces audio. Applications consume the resulting
        // Audio/Source node as a capture device.
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::MEDIA_CLASS => "Audio/Source",
        *pw::keys::NODE_VIRTUAL => "true",
        *pw::keys::NODE_NAME => "openspeechbridge-virtual-mic",
        *pw::keys::NODE_DESCRIPTION => name,
        "priority.session" => "0",
        "priority.driver" => "0",
        "state.restore-props" => "false",
    };

    // Create the virtual mic stream
    let stream = pw::stream::Stream::new(&core, "osb-virtual-mic", stream_props).map_err(|e| {
        PipeWireError::StreamCreationFailed(format!("failed to create virtual mic stream: {}", e))
    })?;

    // Prepare user data for callbacks
    let user_data = VirtualMicUserData { consumer, metrics };

    // Clone references for callbacks
    let mainloop_weak = mainloop.downgrade();

    // Register stream listener with callbacks
    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .state_changed(move |stream, _user_data, old, new| {
            debug!(?old, ?new, "virtual mic stream state changed");

            match &new {
                pw::stream::StreamState::Error(err) => {
                    error!(error = %err, "virtual mic stream error");
                    // Quit the main loop on error
                    if let Some(ml) = mainloop_weak.upgrade() {
                        ml.quit();
                    }
                }
                pw::stream::StreamState::Unconnected => {
                    warn!("virtual mic stream disconnected");
                }
                pw::stream::StreamState::Streaming => {
                    // Get and store the node ID when streaming starts
                    let id = stream.node_id();
                    info!(node_id = id, "virtual mic stream now streaming");
                }
                _ => {}
            }
        })
        .process(move |stream, user_data| {
            // REAL-TIME SAFE CALLBACK
            // This runs in PipeWire's real-time audio thread.
            // Only lock-free operations allowed here.

            if let Some(mut buffer) = stream.dequeue_buffer() {
                user_data.metrics.callbacks.fetch_add(1, Ordering::Relaxed);
                let requested_frames = buffer.requested() as usize;
                let datas = buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    let initial_offset = data.chunk().offset() as u64;
                    let initial_size = data.chunk().size() as u64;
                    if let Some(output) = data.data() {
                        let maxsize = output.len() as u64;
                        let frame_stride = channels as usize * 4;
                        let output_bytes =
                            requested_output_bytes(output.len(), requested_frames, frame_stride);
                        let render = render_virtual_mic_output(
                            &mut user_data.consumer,
                            &mut output[..output_bytes],
                            channels as usize,
                            &user_data.metrics,
                        );
                        user_data
                            .metrics
                            .frames_requested
                            .fetch_add(render.frames_requested, Ordering::Relaxed);
                        user_data
                            .metrics
                            .frames_consumed
                            .fetch_add(render.frames_consumed, Ordering::Relaxed);

                        let frame_stride = channels as u64 * 4;
                        let chunk = data.chunk_mut();
                        *chunk.offset_mut() = 0;
                        *chunk.stride_mut() = frame_stride as i32;
                        *chunk.size_mut() = render.bytes_written as u32;

                        user_data
                            .metrics
                            .initial_offset
                            .store(initial_offset, Ordering::Relaxed);
                        user_data
                            .metrics
                            .initial_size
                            .store(initial_size, Ordering::Relaxed);
                        user_data.metrics.maxsize.store(maxsize, Ordering::Relaxed);
                        user_data
                            .metrics
                            .requested_frames
                            .store(requested_frames as u64, Ordering::Relaxed);
                        user_data
                            .metrics
                            .published_offset
                            .store(0, Ordering::Relaxed);
                        user_data
                            .metrics
                            .published_size
                            .store(render.bytes_written as u64, Ordering::Relaxed);
                        user_data
                            .metrics
                            .published_stride
                            .store(frame_stride, Ordering::Relaxed);
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

    // The virtual source publishes buffers to capture clients. PipeWire calls
    // an Output stream with writable buffers, which we fill and then publish.
    stream
        .connect(
            VIRTUAL_MIC_STREAM_DIRECTION,
            None, // No specific target, apps will connect to us
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| {
            PipeWireError::StreamCreationFailed(format!(
                "failed to connect virtual mic stream: {}",
                e
            ))
        })?;

    // Store the node ID
    node_id.store(stream.node_id(), Ordering::Release);

    info!(
        name = %name,
        node_id = stream.node_id(),
        "virtual mic stream connected, entering main loop"
    );

    // Set up a timer to check is_running flag periodically
    let _timer = setup_stop_timer(&mainloop, is_running, 100)?;

    // Run the main loop (blocks until quit)
    mainloop.run();

    // Cleanup
    let _ = stream.disconnect();

    info!(name = %name, "virtual mic loop exiting");
    Ok(())
}

impl Drop for VirtualMicrophone {
    fn drop(&mut self) {
        if self.is_running.load(Ordering::Acquire) {
            if let Err(e) = self.stop() {
                error!(name = %self.name, error = %e, "failed to stop virtual microphone on drop");
            }
        }
    }
}

/// A virtual audio sink that captures audio from applications.
///
/// This allows capturing the audio output of a specific application
/// or the entire desktop audio for translation of incoming speech.
///
/// # Architecture
///
/// ```text
/// [Apps] --> [PipeWire] --> [VirtualSink Thread] --> [Ring Buffer] --> [Consumer]
///                                   |                                      ^
///                            [Dedicated Thread]                    (returned to caller)
///                            (owns Producer)
/// ```
///
/// - Applications send audio to the virtual sink via PipeWire
/// - The VirtualSink runs a thread that writes to the ring buffer
/// - The consumer (returned by `new()`) receives the captured audio
pub struct VirtualSink {
    /// Display name for the virtual sink.
    name: String,
    /// Audio format specification.
    format: AudioFormat,
    /// Thread-safe running state flag.
    is_running: Arc<AtomicBool>,
    /// PipeWire node ID when registered.
    node_id: Arc<AtomicU32>,
    /// Thread handle for the PipeWire main loop.
    thread_handle: Option<std::thread::JoinHandle<()>>,
    /// Producer for stub mode (before start). None after start().
    stub_producer: Option<AudioRingProducer>,
}

impl VirtualSink {
    /// Create a new virtual sink.
    ///
    /// # Arguments
    ///
    /// * `ctx` - PipeWire context
    /// * `name` - Name for the virtual sink
    /// * `buffer_frames` - Ring buffer size
    ///
    /// # Returns
    ///
    /// A tuple of `(VirtualSink, AudioRingConsumer)`. Audio from applications
    /// will be available from the consumer for processing.
    pub fn new(
        _ctx: &PipeWireContext,
        name: &str,
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
            name = %name,
            buffer_frames = buffer_frames,
            "creating virtual sink"
        );

        let sink = Self {
            name: name.to_string(),
            format,
            is_running: Arc::new(AtomicBool::new(false)),
            node_id: Arc::new(AtomicU32::new(0)),
            thread_handle: None,
            stub_producer: Some(producer),
        };

        Ok((sink, consumer))
    }

    /// Get the name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the format.
    #[inline]
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Get node ID.
    #[inline]
    pub fn node_id(&self) -> Option<u32> {
        let id = self.node_id.load(Ordering::Acquire);
        if id == 0 {
            None
        } else {
            Some(id)
        }
    }

    /// Check if running.
    #[inline]
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    /// Start the virtual sink.
    ///
    /// Creates a PipeWire node with `media.class = "Audio/Sink"` that
    /// applications can send audio to.
    pub fn start(&mut self) -> Result<()> {
        if self.is_running.load(Ordering::Acquire) {
            return Ok(());
        }

        info!(name = %self.name, "starting virtual sink");

        // Take the producer - it will be moved to the PipeWire thread
        let producer = match self.stub_producer.take() {
            Some(p) => p,
            None => {
                warn!(name = %self.name, "virtual sink already started or producer consumed");
                return Ok(());
            }
        };

        // Clone shared state for the thread
        let is_running = Arc::clone(&self.is_running);
        let node_id = Arc::clone(&self.node_id);
        let name = self.name.clone();
        let format = self.format;

        // Spawn the PipeWire thread
        let handle = std::thread::Builder::new()
            .name(format!("pw-vsink-{}", name))
            .spawn(move || {
                if let Err(e) = run_virtual_sink_loop(producer, &name, format, is_running, node_id)
                {
                    error!(name = %name, error = %e, "virtual sink loop failed");
                }
            })
            .map_err(|e| PipeWireError::Internal(format!("failed to spawn thread: {}", e)))?;

        self.thread_handle = Some(handle);
        self.is_running.store(true, Ordering::Release);

        info!(name = %self.name, "virtual sink started");
        Ok(())
    }

    /// Stop the virtual sink.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running.load(Ordering::Acquire) {
            return Ok(());
        }

        info!(name = %self.name, "stopping virtual sink");
        self.is_running.store(false, Ordering::Release);

        if let Some(handle) = self.thread_handle.take() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            match handle.join() {
                Ok(()) => debug!(name = %self.name, "virtual sink thread joined"),
                Err(_) => warn!(name = %self.name, "virtual sink thread panicked"),
            }
        }

        self.node_id.store(0, Ordering::Release);
        Ok(())
    }

    /// Write samples received from applications (stub mode only).
    ///
    /// This method is only available before `start()` is called.
    ///
    /// # Real-time Safety
    ///
    /// This method is real-time safe - uses lock-free ring buffer.
    pub fn write_samples(&mut self, samples: &[f32]) -> usize {
        if let Some(ref mut producer) = self.stub_producer {
            producer.push(samples)
        } else {
            0
        }
    }
}

/// User data passed to PipeWire stream callbacks for virtual sink.
///
/// This struct is owned by the PipeWire listener and provides access to
/// the ring buffer producer within the process callback.
///
/// # Real-time Safety
///
/// All fields used in the process callback must support lock-free operations:
/// - `producer`: uses `rtrb` lock-free SPSC ring buffer
struct VirtualSinkUserData {
    /// Lock-free ring buffer producer for audio samples.
    producer: AudioRingProducer,
}

/// Run the PipeWire virtual sink loop in a dedicated thread.
///
/// This function owns the `AudioRingProducer` and receives audio from PipeWire.
/// It creates a real PipeWire stream with `media.class = "Audio/Sink"` that
/// applications can send audio to.
///
/// # PipeWire Integration
///
/// 1. Creates a `MainLoop` for event processing
/// 2. Creates a `Context` and connects to PipeWire daemon
/// 3. Creates a `Stream` with virtual audio sink properties
/// 4. Registers a real-time safe `process` callback
/// 5. Connects with `Direction::Input` to receive audio
/// 6. Runs the main loop until `is_running` is set to false
///
/// # Real-time Safety
///
/// The `process` callback only uses:
/// - `producer.push()`: lock-free ring buffer write
/// - No allocations, no locks, no I/O
fn run_virtual_sink_loop(
    producer: AudioRingProducer,
    name: &str,
    format: AudioFormat,
    is_running: Arc<AtomicBool>,
    node_id: Arc<AtomicU32>,
) -> Result<()> {
    // Initialize PipeWire for this thread
    pw::init();

    let channels = format.channels.channels() as u32;
    let rate = format.sample_rate.hz();

    info!(
        name = %name,
        channels = channels,
        rate = rate,
        "virtual sink loop starting with real PipeWire integration"
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

    info!(name = %name, "connected to PipeWire daemon for virtual sink");

    // Build stream properties for virtual audio sink
    // media.class = "Audio/Sink" makes this appear as an audio output device
    let stream_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Playback",  // From app's perspective, this is a playback device
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::MEDIA_CLASS => "Audio/Sink",
        *pw::keys::NODE_NAME => "openspeechbridge-virtual-sink",
        *pw::keys::NODE_DESCRIPTION => name,
    };

    // Create the virtual sink stream
    let stream = pw::stream::Stream::new(&core, "osb-virtual-sink", stream_props).map_err(|e| {
        PipeWireError::StreamCreationFailed(format!("failed to create virtual sink stream: {}", e))
    })?;

    // Prepare user data for callbacks
    let user_data = VirtualSinkUserData { producer };

    // Clone references for callbacks
    let mainloop_weak = mainloop.downgrade();

    // Register stream listener with callbacks
    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .state_changed(move |stream, _user_data, old, new| {
            debug!(?old, ?new, "virtual sink stream state changed");

            match &new {
                pw::stream::StreamState::Error(err) => {
                    error!(error = %err, "virtual sink stream error");
                    // Quit the main loop on error
                    if let Some(ml) = mainloop_weak.upgrade() {
                        ml.quit();
                    }
                }
                pw::stream::StreamState::Unconnected => {
                    warn!("virtual sink stream disconnected");
                }
                pw::stream::StreamState::Streaming => {
                    let id = stream.node_id();
                    info!(node_id = id, "virtual sink stream now streaming");
                }
                _ => {}
            }
        })
        .process(|stream, user_data| {
            // REAL-TIME SAFE CALLBACK
            // This runs in PipeWire's real-time audio thread.
            // Only lock-free operations allowed here.

            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    let offset = data.chunk().offset() as usize;
                    let size = data.chunk().size() as usize;

                    if let Some(slice) = data.data() {
                        if offset + size <= slice.len() {
                            let audio_bytes = &slice[offset..offset + size];

                            // Process in batches using stack buffer (no allocation)
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

                                if pushed < converted {
                                    // Buffer full, stop processing this frame
                                    break;
                                }

                                byte_offset += converted * 4;
                            }
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

    // Connect the stream (Direction::Input for receiving audio from apps)
    stream
        .connect(
            spa::utils::Direction::Input,
            None, // No specific source, apps will connect to us
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| {
            PipeWireError::StreamCreationFailed(format!(
                "failed to connect virtual sink stream: {}",
                e
            ))
        })?;

    // Store the node ID
    node_id.store(stream.node_id(), Ordering::Release);

    info!(
        name = %name,
        node_id = stream.node_id(),
        "virtual sink stream connected, entering main loop"
    );

    // Set up a timer to check is_running flag periodically
    let _timer = setup_stop_timer(&mainloop, is_running, 100)?;

    // Run the main loop (blocks until quit)
    mainloop.run();

    // Cleanup
    let _ = stream.disconnect();

    info!(name = %name, "virtual sink loop exiting");
    Ok(())
}

impl Drop for VirtualSink {
    fn drop(&mut self) {
        if self.is_running.load(Ordering::Acquire) {
            if let Err(e) = self.stop() {
                error!(name = %self.name, error = %e, "failed to stop virtual sink on drop");
            }
        }
    }
}

/// Default name for the OpenSpeechBridge virtual microphone.
pub const DEFAULT_VIRTUAL_MIC_NAME: &str = "OpenSpeechBridge Virtual Microphone";

/// Default name for the OpenSpeechBridge virtual sink.
pub const DEFAULT_VIRTUAL_SINK_NAME: &str = "OpenSpeechBridge Virtual Sink";

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_nonzero_output_survives_ring_and_spa_copy(channels: usize) {
        let frames = 128usize;
        let (mut producer, mut consumer) = AudioRingBuffer::new(frames as u32, channels as u8);
        let input: Vec<f32> = (0..frames * channels)
            .map(|index| if index % 2 == 0 { 0.25 } else { -0.125 })
            .collect();
        assert_eq!(producer.push(&input), input.len());

        let metrics = VirtualMicMetrics::default();
        metrics
            .signal_metrics_enabled
            .store(true, Ordering::Relaxed);
        let mut output = vec![0u8; input.len() * std::mem::size_of::<f32>()];
        let rendered = render_virtual_mic_output(&mut consumer, &mut output, channels, &metrics);

        assert_eq!(rendered.frames_requested, frames as u64);
        assert_eq!(rendered.frames_consumed, frames as u64);
        assert_eq!(rendered.bytes_written, output.len());

        let mut decoded = vec![0.0f32; input.len()];
        assert_eq!(bytes_to_f32_samples(&output, &mut decoded), input.len());
        assert_eq!(decoded, input);
        assert!(decoded.iter().any(|sample| *sample != 0.0));

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.ring_output.nonzero, input.len() as u64);
        assert_eq!(snapshot.output_before_copy.nonzero, input.len() as u64);
        assert_eq!(snapshot.output_after_copy.nonzero, input.len() as u64);
        assert!(snapshot.ring_output.rms > 0.0);
        assert!(snapshot.output_after_copy.peak > 0.0);
    }

    #[test]
    fn virtual_mic_stream_publishes_pipewire_output() {
        assert_eq!(VIRTUAL_MIC_STREAM_DIRECTION, spa::utils::Direction::Output);
    }

    #[test]
    fn virtual_mic_preserves_nonzero_mono_signal() {
        assert_nonzero_output_survives_ring_and_spa_copy(1);
    }

    #[test]
    fn virtual_mic_preserves_nonzero_stereo_signal() {
        assert_nonzero_output_survives_ring_and_spa_copy(2);
    }

    #[test]
    fn virtual_mic_limits_output_to_requested_frames() {
        assert_eq!(requested_output_bytes(131_072, 1_024, 8), 8_192);
        assert_eq!(requested_output_bytes(4_096, 1_024, 8), 4_096);
        assert_eq!(requested_output_bytes(4_096, 0, 8), 4_096);
    }

    /// Test that VirtualMicrophone can be created with default settings.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_virtual_mic_creation() {
        let ctx = PipeWireContext::new().unwrap();
        let (vmic, _producer) = VirtualMicrophone::new(&ctx, "Test Mic", 4096).unwrap();
        assert_eq!(vmic.name(), "Test Mic");
        assert!(!vmic.is_running());
        assert!(vmic.node_id().is_none());
    }

    /// Test that VirtualMicrophone respects zero buffer_frames parameter.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_virtual_mic_default_buffer() {
        let ctx = PipeWireContext::new().unwrap();
        let (vmic, _producer) = VirtualMicrophone::new(&ctx, "Test Mic", 0).unwrap();
        // Should use default buffer size
        assert!(!vmic.is_running());
    }

    /// Test VirtualSink creation.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_virtual_sink_creation() {
        let ctx = PipeWireContext::new().unwrap();
        let (sink, _consumer) = VirtualSink::new(&ctx, "Test Sink", 4096).unwrap();
        assert_eq!(sink.name(), "Test Sink");
        assert!(!sink.is_running());
    }

    /// Test that read_samples fills with silence on empty buffer.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_virtual_mic_read_silence() {
        let ctx = PipeWireContext::new().unwrap();
        let (mut vmic, _producer) = VirtualMicrophone::new(&ctx, "Test Mic", 4096).unwrap();

        let mut output = vec![1.0f32; 256];
        let read = vmic.read_samples(&mut output);

        // Should fill with silence (zeros)
        assert_eq!(read, 0);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    /// Test that data flows through the ring buffer correctly.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_virtual_mic_data_flow() {
        let ctx = PipeWireContext::new().unwrap();
        let (mut vmic, mut producer) = VirtualMicrophone::new(&ctx, "Test Mic", 4096).unwrap();

        // Write some samples
        let input = vec![0.5f32; 256];
        let written = producer.push(&input);
        assert_eq!(written, 256);

        // Read them back
        let mut output = vec![0.0f32; 256];
        let read = vmic.read_samples(&mut output);

        assert_eq!(read, 256);
        assert_eq!(input, output);
    }

    /// Test format accessor.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_virtual_mic_format() {
        let ctx = PipeWireContext::new().unwrap();
        let (vmic, _producer) = VirtualMicrophone::new(&ctx, "Test Mic", 4096).unwrap();

        let format = vmic.format();
        assert_eq!(format.sample_rate, SampleRate::PRO_48K);
        assert_eq!(format.sample_format, SampleFormat::F32);
        assert_eq!(format.channels, ChannelLayout::Stereo);
    }

    /// Test buffer occupancy reporting.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_virtual_mic_buffer_occupancy() {
        let ctx = PipeWireContext::new().unwrap();
        let (vmic, mut producer) = VirtualMicrophone::new(&ctx, "Test Mic", 1000).unwrap();

        // Empty buffer
        assert!(vmic.buffer_occupancy() < 0.01);

        // Add some data (500 frames = 1000 samples for stereo)
        producer.push(&vec![0.5f32; 1000]);

        // Should be ~50% full
        let occupancy = vmic.buffer_occupancy();
        assert!(occupancy > 0.4 && occupancy < 0.6);
    }
}
