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

use super::context::PipeWireContext;
use crate::error::{PipeWireError, Result};
use osb_audio::buffer::{AudioRingBuffer, AudioRingConsumer, AudioRingProducer};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Default buffer size in frames for virtual devices.
const DEFAULT_BUFFER_FRAMES: u32 = 4096;

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

    /// Start the virtual microphone.
    ///
    /// This creates a PipeWire node with `media.class = "Audio/Source/Virtual"`
    /// and registers it with the PipeWire graph. The device will appear in
    /// application device lists (Discord, Zoom, etc.) as an available microphone.
    ///
    /// The `AudioRingConsumer` is moved to a dedicated thread where it will
    /// be used by the PipeWire process callback.
    ///
    /// # PipeWire Node Properties
    ///
    /// - `media.class = "Audio/Source/Virtual"` - Identifies as virtual audio source
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
        let name = self.name.clone();
        let format = self.format;

        // Spawn the PipeWire thread
        let handle = std::thread::Builder::new()
            .name(format!("pw-vmic-{}", name))
            .spawn(move || {
                if let Err(e) = run_virtual_mic_loop(consumer, &name, format, is_running, node_id) {
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
}

/// Run the PipeWire virtual microphone loop in a dedicated thread.
///
/// This function owns the `AudioRingConsumer` and provides audio to PipeWire.
/// It creates a real PipeWire stream with `media.class = "Audio/Source/Virtual"`
/// that appears as a microphone in applications.
///
/// # PipeWire Integration
///
/// 1. Creates a `MainLoop` for event processing
/// 2. Creates a `Context` and connects to PipeWire daemon
/// 3. Creates a `Stream` with virtual audio source properties
/// 4. Registers a real-time safe `process` callback
/// 5. Connects with `Direction::Output` to provide audio
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
    // media.class = "Audio/Source/Virtual" makes this appear as a microphone
    let stream_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",  // From app's perspective, this is a capture device
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::MEDIA_CLASS => "Audio/Source/Virtual",
        *pw::keys::NODE_NAME => "openspeechbridge-virtual-mic",
        *pw::keys::NODE_DESCRIPTION => name,
    };

    // Create the virtual mic stream
    let stream = pw::stream::Stream::new(&core, "osb-virtual-mic", stream_props).map_err(|e| {
        PipeWireError::StreamCreationFailed(format!("failed to create virtual mic stream: {}", e))
    })?;

    // Prepare user data for callbacks
    let user_data = VirtualMicUserData { consumer };

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
        .process(|stream, user_data| {
            // REAL-TIME SAFE CALLBACK
            // This runs in PipeWire's real-time audio thread.
            // Only lock-free operations allowed here.

            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    if let Some(slice) = data.data() {
                        // Calculate how many samples we can write
                        let num_samples = slice.len() / 4; // 4 bytes per f32

                        // Use a stack buffer to read from ring buffer
                        // Then convert to bytes for PipeWire
                        let mut sample_buf = [0.0f32; 512]; // Stack buffer, no allocation
                        let mut byte_offset = 0usize;

                        while byte_offset + 4 <= slice.len() {
                            let batch_samples =
                                ((slice.len() - byte_offset) / 4).min(sample_buf.len());

                            // Read from ring buffer (lock-free, real-time safe)
                            // pop_or_silence fills with zeros if buffer is empty
                            let _read = user_data
                                .consumer
                                .pop_or_silence(&mut sample_buf[..batch_samples]);

                            // Convert f32 samples to F32LE bytes
                            for (i, &sample) in sample_buf[..batch_samples].iter().enumerate() {
                                let bytes = sample.to_le_bytes();
                                let idx = byte_offset + i * 4;
                                if idx + 4 <= slice.len() {
                                    slice[idx] = bytes[0];
                                    slice[idx + 1] = bytes[1];
                                    slice[idx + 2] = bytes[2];
                                    slice[idx + 3] = bytes[3];
                                }
                            }

                            byte_offset += batch_samples * 4;
                        }

                        // Update chunk metadata to indicate how much data we wrote
                        let chunk = data.chunk_mut();
                        *chunk.offset_mut() = 0;
                        *chunk.stride_mut() = 4; // 4 bytes per sample for interleaved F32
                        *chunk.size_mut() = (num_samples * 4) as u32;
                    }
                }
                // Buffer is automatically queued back when dropped
            }
        })
        .register()
        .map_err(|e| {
            PipeWireError::StreamCreationFailed(format!("failed to register listener: {}", e))
        })?;

    // Build audio format parameters
    let spa_format = match format.sample_format {
        SampleFormat::F32 => spa::param::audio::AudioFormat::F32LE,
        SampleFormat::I16 => spa::param::audio::AudioFormat::S16LE,
        SampleFormat::I32 => spa::param::audio::AudioFormat::S32LE,
        _ => spa::param::audio::AudioFormat::F32LE, // Default to F32
    };

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa_format);
    audio_info.set_rate(rate);
    audio_info.set_channels(channels);

    // Serialize audio format to POD
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: pw::spa::sys::SPA_TYPE_OBJECT_Format,
            id: pw::spa::sys::SPA_PARAM_EnumFormat,
            properties: audio_info.into(),
        }),
    )
    .map_err(|e| PipeWireError::Internal(format!("failed to serialize audio format: {:?}", e)))?
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values)
        .ok_or_else(|| PipeWireError::Internal("failed to create format pod".to_string()))?];

    // Connect the stream (Direction::Output for providing audio to apps)
    stream
        .connect(
            spa::utils::Direction::Output,
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
    let is_running_timer = Arc::clone(&is_running);
    let mainloop_for_timer = mainloop.downgrade();
    let _timer = mainloop.loop_().add_timer(move |_| {
        if !is_running_timer.load(Ordering::Acquire) {
            if let Some(ml) = mainloop_for_timer.upgrade() {
                ml.quit();
            }
        }
    });
    _timer
        .update_timer(
            Some(std::time::Duration::from_millis(100)),
            Some(std::time::Duration::from_millis(100)),
        )
        .into_result()
        .map_err(|e| PipeWireError::Internal(format!("failed to set timer: {:?}", e)))?;

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
                    // Read chunk info first (immutable borrow)
                    let offset = data.chunk().offset() as usize;
                    let size = data.chunk().size() as usize;

                    // Now get mutable access to the data slice
                    if let Some(slice) = data.data() {
                        // Bounds check
                        if offset + size <= slice.len() {
                            let audio_bytes = &slice[offset..offset + size];

                            // Convert bytes to f32 samples (F32LE format)
                            let mut sample_buf = [0.0f32; 512]; // Stack buffer, no allocation
                            let mut byte_offset = 0usize;

                            while byte_offset + 4 <= audio_bytes.len() {
                                let batch_size =
                                    ((audio_bytes.len() - byte_offset) / 4).min(sample_buf.len());

                                // Note: Using index loop here because we need both:
                                // 1. Index for sample_buf assignment
                                // 2. Calculated index into audio_bytes based on byte_offset
                                #[allow(clippy::needless_range_loop)]
                                for i in 0..batch_size {
                                    let idx = byte_offset + i * 4;
                                    if idx + 4 <= audio_bytes.len() {
                                        let bytes: [u8; 4] = [
                                            audio_bytes[idx],
                                            audio_bytes[idx + 1],
                                            audio_bytes[idx + 2],
                                            audio_bytes[idx + 3],
                                        ];
                                        sample_buf[i] = f32::from_le_bytes(bytes);
                                    }
                                }

                                // Push batch to ring buffer (lock-free, real-time safe)
                                let pushed = user_data.producer.push(&sample_buf[..batch_size]);

                                if pushed < batch_size {
                                    // Buffer full, stop processing this frame
                                    break;
                                }

                                byte_offset += batch_size * 4;
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

    // Build audio format parameters
    let spa_format = match format.sample_format {
        SampleFormat::F32 => spa::param::audio::AudioFormat::F32LE,
        SampleFormat::I16 => spa::param::audio::AudioFormat::S16LE,
        SampleFormat::I32 => spa::param::audio::AudioFormat::S32LE,
        _ => spa::param::audio::AudioFormat::F32LE, // Default to F32
    };

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa_format);
    audio_info.set_rate(rate);
    audio_info.set_channels(channels);

    // Serialize audio format to POD
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: pw::spa::sys::SPA_TYPE_OBJECT_Format,
            id: pw::spa::sys::SPA_PARAM_EnumFormat,
            properties: audio_info.into(),
        }),
    )
    .map_err(|e| PipeWireError::Internal(format!("failed to serialize audio format: {:?}", e)))?
    .0
    .into_inner();

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
    let is_running_timer = Arc::clone(&is_running);
    let mainloop_for_timer = mainloop.downgrade();
    let _timer = mainloop.loop_().add_timer(move |_| {
        if !is_running_timer.load(Ordering::Acquire) {
            if let Some(ml) = mainloop_for_timer.upgrade() {
                ml.quit();
            }
        }
    });
    _timer
        .update_timer(
            Some(std::time::Duration::from_millis(100)),
            Some(std::time::Duration::from_millis(100)),
        )
        .into_result()
        .map_err(|e| PipeWireError::Internal(format!("failed to set timer: {:?}", e)))?;

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
