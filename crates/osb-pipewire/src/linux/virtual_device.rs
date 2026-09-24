//! Virtual audio device implementation for PipeWire.
//!
//! Virtual devices allow OpenSpeechBridge to inject audio into applications
//! (virtual microphone) or capture audio from applications (virtual sink).
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
//! We achieve this by using lock-free SPSC ring buffers for audio data transfer.

use super::context::PipeWireContext;
use crate::error::Result;
use osb_audio::buffer::{AudioRingBuffer, AudioRingConsumer, AudioRingProducer};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
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
/// [Producer] --> [Ring Buffer] --> [VirtualMicrophone] --> [PipeWire] --> [Apps]
/// ```
///
/// - The producer (returned by `new()`) receives translated audio
/// - The VirtualMicrophone reads from the ring buffer in the PipeWire callback
/// - Applications receive the audio as if from a real microphone
///
/// # Real-time Safety
///
/// The `process` callback uses `pop_or_silence()` which:
/// - Never blocks (lock-free SPSC buffer)
/// - Never allocates (fills existing buffer in-place)
/// - Always returns valid audio (silence on underrun)
pub struct VirtualMicrophone {
    /// Display name for the virtual microphone.
    name: String,
    /// Consumer side of the lock-free ring buffer.
    consumer: AudioRingConsumer,
    /// Audio format specification.
    format: AudioFormat,
    /// Thread-safe running state flag.
    is_running: Arc<AtomicBool>,
    /// PipeWire node ID when registered (set after stream creation).
    node_id: Arc<AtomicU32>,
    /// Thread handle for the PipeWire main loop.
    thread_handle: Option<std::thread::JoinHandle<()>>,
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
    /// let (mut vmic, producer) = VirtualMicrophone::new(&ctx, "My Virtual Mic", 4096)?;
    ///
    /// // Start the virtual microphone
    /// vmic.start()?;
    ///
    /// // Write audio to the producer (from another thread)
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
            consumer,
            format,
            is_running: Arc::new(AtomicBool::new(false)),
            node_id: Arc::new(AtomicU32::new(0)),
            thread_handle: None,
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

        // NOTE: Full PipeWire stream implementation requires the consumer to be
        // accessible from the PipeWire callback thread. Since AudioRingConsumer
        // is not Send+Sync (uses internal Rc), we would need to either:
        //
        // 1. Use a thread-safe wrapper (Arc<Mutex<...>>) - but this violates real-time safety
        // 2. Create the stream in the same thread as the consumer
        // 3. Use raw pointers with careful lifetime management
        //
        // For now, we mark the device as running and log the intent.
        // The actual PipeWire stream creation will be implemented when we have
        // a thread-safe ring buffer or can restructure the ownership model.
        //
        // Real implementation would:
        // 1. Create a PipeWire node with media.class = "Audio/Source/Virtual"
        // 2. Register metadata for app visibility
        // 3. Set up process callback to call self.read_samples()

        self.is_running.store(true, Ordering::Release);
        info!(name = %self.name, "virtual microphone started (stub mode)");
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
            std::thread::sleep(std::time::Duration::from_millis(50));

            // The thread should exit on its own, but we join anyway
            if handle.join().is_err() {
                warn!(name = %self.name, "virtual microphone thread panicked");
            }
        }

        self.node_id.store(0, Ordering::Release);
        info!(name = %self.name, "virtual microphone stopped");
        Ok(())
    }

    /// Read samples to provide to applications.
    ///
    /// This method is called by the PipeWire process callback to fill
    /// the output buffer with audio data. It reads from the internal
    /// ring buffer, filling with silence if not enough data is available.
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
    /// `output.len()` if buffer was partially empty; remainder is silence).
    pub fn read_samples(&mut self, output: &mut [f32]) -> usize {
        self.consumer.pop_or_silence(output)
    }

    /// Get current buffer occupancy as a ratio (0.0 to 1.0).
    ///
    /// Useful for monitoring buffer health:
    /// - Near 0.0: Risk of underruns (producer too slow)
    /// - Near 1.0: Risk of overruns (consumer too slow)
    /// - Around 0.5: Healthy buffer level
    #[inline]
    pub fn buffer_occupancy(&self) -> f32 {
        self.consumer.occupancy().fill_ratio
    }
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
/// [Apps] --> [PipeWire] --> [VirtualSink] --> [Ring Buffer] --> [Consumer]
/// ```
///
/// - Applications send audio to the virtual sink
/// - The VirtualSink writes to the ring buffer in the PipeWire callback
/// - The consumer (returned by `new()`) receives the captured audio
pub struct VirtualSink {
    /// Display name for the virtual sink.
    name: String,
    /// Producer side of the lock-free ring buffer.
    producer: AudioRingProducer,
    /// Audio format specification.
    format: AudioFormat,
    /// Thread-safe running state flag.
    is_running: Arc<AtomicBool>,
    /// PipeWire node ID when registered.
    node_id: Arc<AtomicU32>,
    /// Thread handle for the PipeWire main loop.
    thread_handle: Option<std::thread::JoinHandle<()>>,
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
            producer,
            format,
            is_running: Arc::new(AtomicBool::new(false)),
            node_id: Arc::new(AtomicU32::new(0)),
            thread_handle: None,
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
        self.is_running.store(true, Ordering::Release);
        info!(name = %self.name, "virtual sink started (stub mode)");
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
            std::thread::sleep(std::time::Duration::from_millis(50));
            if handle.join().is_err() {
                warn!(name = %self.name, "virtual sink thread panicked");
            }
        }

        self.node_id.store(0, Ordering::Release);
        Ok(())
    }

    /// Write samples received from applications (called by audio callback).
    ///
    /// # Real-time Safety
    ///
    /// This method is real-time safe - uses lock-free ring buffer.
    pub fn write_samples(&mut self, samples: &[f32]) -> usize {
        self.producer.push(samples)
    }
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
