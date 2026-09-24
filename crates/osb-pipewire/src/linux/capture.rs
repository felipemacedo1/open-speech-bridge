//! Audio capture stream implementation for PipeWire.
//!
//! This module provides real-time audio capture from PipeWire devices
//! using lock-free ring buffers for thread-safe audio data transfer.
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info};

/// Default buffer size in frames for capture streams.
const DEFAULT_BUFFER_FRAMES: u32 = 4096;

/// Stream state values (mirrors PipeWire stream states).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum StreamState {
    /// Stream is in error state.
    Error = 0,
    /// Stream is unconnected.
    Unconnected = 1,
    /// Stream is connecting.
    Connecting = 2,
    /// Stream is paused.
    Paused = 3,
    /// Stream is actively streaming.
    Streaming = 4,
}

impl From<u32> for StreamState {
    fn from(value: u32) -> Self {
        match value {
            0 => StreamState::Error,
            1 => StreamState::Unconnected,
            2 => StreamState::Connecting,
            3 => StreamState::Paused,
            4 => StreamState::Streaming,
            _ => StreamState::Error,
        }
    }
}

/// Audio capture stream for recording from a PipeWire device.
///
/// # Architecture
///
/// ```text
/// [PipeWire Device] --> [CaptureStream] --> [Ring Buffer] --> [Consumer]
/// ```
///
/// The capture stream connects to a PipeWire audio source (microphone) and
/// pushes captured samples to a lock-free ring buffer. The consumer can
/// read from the buffer without blocking the real-time audio thread.
///
/// # Real-time Safety
///
/// The PipeWire process callback uses `producer.push()` which:
/// - Never blocks (lock-free SPSC buffer)
/// - Never allocates (uses pre-allocated buffer)
/// - Records overflow metrics if buffer is full
pub struct CaptureStream {
    /// Producer side of the lock-free ring buffer.
    producer: AudioRingProducer,
    /// Audio format specification.
    format: AudioFormat,
    /// Target device identifier.
    device_id: String,
    /// Thread-safe running state flag.
    is_running: Arc<AtomicBool>,
    /// Device disconnection flag.
    device_disconnected: Arc<AtomicBool>,
    /// Samples captured counter (atomic for cross-thread access).
    samples_captured: Arc<AtomicU64>,
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
            producer,
            format,
            device_id: device_id.to_string(),
            is_running: Arc::new(AtomicBool::new(false)),
            device_disconnected: Arc::new(AtomicBool::new(false)),
            samples_captured: Arc::new(AtomicU64::new(0)),
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
            producer,
            format,
            device_id: device_id.to_string(),
            is_running: Arc::new(AtomicBool::new(false)),
            device_disconnected: Arc::new(AtomicBool::new(false)),
            samples_captured: Arc::new(AtomicU64::new(0)),
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

    /// Start capturing audio.
    ///
    /// This begins the audio capture from the specified device. Samples
    /// will be pushed to the ring buffer and can be read from the consumer
    /// returned by `new()`.
    ///
    /// # PipeWire Integration
    ///
    /// In the full implementation, this method:
    /// 1. Creates a PipeWire stream with `pw::stream::Stream`
    /// 2. Configures the audio format (F32LE, 48kHz, Stereo)
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

        // NOTE: Full PipeWire stream implementation
        //
        // The complete implementation would create a PipeWire stream like this:
        //
        // ```rust
        // let mainloop = pw::thread_loop::ThreadLoop::new(None, None)?;
        // let context = pw::context::Context::new(&mainloop)?;
        // let core = context.connect(None)?;
        //
        // let stream = pw::stream::Stream::new(
        //     &core,
        //     "osb-capture",
        //     properties! {
        //         *pw::keys::MEDIA_TYPE => "Audio",
        //         *pw::keys::MEDIA_CATEGORY => "Capture",
        //         *pw::keys::MEDIA_ROLE => "Communication",
        //     },
        // )?;
        //
        // // In the process callback (REAL-TIME SAFE):
        // // - Get buffer from PipeWire
        // // - Extract samples as &[f32]
        // // - Call self.producer.push(samples) - lock-free!
        // ```
        //
        // Current limitation: AudioRingProducer is not Send+Sync because
        // it uses ringbuf's Producer which contains Rc internally.
        // Options to resolve:
        // 1. Use Arc<AtomicRingBuffer> pattern
        // 2. Run PipeWire stream in same thread as producer
        // 3. Use raw pointer with careful lifetime management

        self.is_running.store(true, Ordering::Release);
        self.device_disconnected.store(false, Ordering::Release);

        info!(device = %self.device_id, "capture stream started (stub mode)");
        Ok(())
    }

    /// Stop capturing audio.
    ///
    /// This stops the PipeWire stream and releases resources. The ring
    /// buffer contents remain available for the consumer to read.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running.load(Ordering::Acquire) {
            debug!(device = %self.device_id, "capture stream already stopped");
            return Ok(());
        }

        info!(device = %self.device_id, "stopping capture stream");

        self.is_running.store(false, Ordering::Release);

        let total_samples = self.samples_captured.load(Ordering::Relaxed);
        info!(
            device = %self.device_id,
            total_samples = total_samples,
            "capture stream stopped"
        );

        Ok(())
    }

    /// Push samples directly to the ring buffer.
    ///
    /// This method is primarily for testing or manual audio injection.
    /// In normal operation, samples are pushed by the PipeWire callback.
    ///
    /// # Real-time Safety
    ///
    /// This method is real-time safe - uses lock-free ring buffer.
    ///
    /// # Returns
    ///
    /// Number of samples actually pushed (may be less than input if buffer full).
    pub fn push_samples(&mut self, samples: &[f32]) -> usize {
        let pushed = self.producer.push(samples);
        self.samples_captured
            .fetch_add(pushed as u64, Ordering::Relaxed);
        pushed
    }

    /// Get the current buffer occupancy as a ratio (0.0 to 1.0).
    ///
    /// Useful for monitoring buffer health:
    /// - Near 0.0: Consumer is keeping up well
    /// - Near 1.0: Risk of overflow (consumer too slow)
    #[inline]
    pub fn buffer_occupancy(&self) -> f32 {
        self.producer.occupancy().fill_ratio
    }

    /// Check if the buffer has space for the given number of frames.
    #[inline]
    pub fn has_space_for(&self, frames: u32) -> bool {
        self.producer.has_space_for(frames)
    }
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
    fn test_stream_state_conversion() {
        assert_eq!(StreamState::from(0), StreamState::Error);
        assert_eq!(StreamState::from(1), StreamState::Unconnected);
        assert_eq!(StreamState::from(2), StreamState::Connecting);
        assert_eq!(StreamState::from(3), StreamState::Paused);
        assert_eq!(StreamState::from(4), StreamState::Streaming);
        assert_eq!(StreamState::from(99), StreamState::Error);
    }

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

    /// Test buffer operations without PipeWire.
    #[test]
    #[ignore = "requires PipeWire daemon"]
    fn test_capture_stream_buffer_ops() {
        let ctx = PipeWireContext::new().unwrap();
        let (mut stream, mut consumer) = CaptureStream::new(&ctx, "default", 1000).unwrap();

        // Push some samples
        let input = vec![0.5f32; 500];
        let pushed = stream.push_samples(&input);
        assert_eq!(pushed, 500);
        assert_eq!(stream.samples_captured(), 500);

        // Check occupancy
        let occupancy = stream.buffer_occupancy();
        assert!(occupancy > 0.2 && occupancy < 0.3); // ~25% for stereo (500 samples / 2000 capacity)

        // Read samples
        let mut output = vec![0.0f32; 500];
        let read = consumer.pop(&mut output);
        assert_eq!(read, 500);
        assert_eq!(input, output);
    }

    /// Test start/stop lifecycle without PipeWire.
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
