//! Audio buffer implementations.
//!
//! This module provides bounded, lock-free buffers for real-time audio processing.
//! The primary buffer type is [`AudioRingBuffer`], a SPSC ring buffer suitable
//! for passing audio between threads without blocking.

use osb_core::metrics::{AudioMetrics, BufferOccupancy};
use ringbuf::{
    traits::{Consumer, Observer, Producer, Split},
    HeapRb,
};
use std::sync::Arc;
use tracing::{debug, warn};

/// A simple audio buffer for temporary storage.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// Sample data (interleaved if multi-channel)
    data: Vec<f32>,
    /// Number of channels
    channels: u8,
}

impl AudioBuffer {
    /// Create a new audio buffer with the specified capacity.
    pub fn new(capacity_frames: usize, channels: u8) -> Self {
        Self {
            data: vec![0.0; capacity_frames * channels as usize],
            channels,
        }
    }

    /// Create a buffer from existing data.
    pub fn from_samples(data: Vec<f32>, channels: u8) -> Self {
        Self { data, channels }
    }

    /// Get the number of frames in the buffer.
    #[inline]
    pub fn frames(&self) -> usize {
        self.data.len() / self.channels as usize
    }

    /// Get the number of channels.
    #[inline]
    pub fn channels(&self) -> u8 {
        self.channels
    }

    /// Get a slice of the sample data.
    #[inline]
    pub fn samples(&self) -> &[f32] {
        &self.data
    }

    /// Get a mutable slice of the sample data.
    #[inline]
    pub fn samples_mut(&mut self) -> &mut [f32] {
        &mut self.data
    }

    /// Clear the buffer (fill with zeros).
    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }

    /// Resize the buffer.
    pub fn resize(&mut self, frames: usize) {
        self.data.resize(frames * self.channels as usize, 0.0);
    }
}

/// Producer half of an audio ring buffer.
pub struct AudioRingProducer {
    inner: ringbuf::HeapProd<f32>,
    capacity_frames: u32,
    channels: u8,
    metrics: Arc<AudioMetrics>,
}

/// Consumer half of an audio ring buffer.
pub struct AudioRingConsumer {
    inner: ringbuf::HeapCons<f32>,
    capacity_frames: u32,
    channels: u8,
    metrics: Arc<AudioMetrics>,
}

/// A lock-free SPSC ring buffer for audio samples.
///
/// This buffer is designed for real-time audio processing where one thread
/// produces samples (e.g., audio capture) and another consumes them
/// (e.g., processing or playback).
///
/// # Example
///
/// ```
/// use osb_audio::AudioRingBuffer;
///
/// let (mut producer, mut consumer) = AudioRingBuffer::new(1024, 2);
///
/// // Producer thread: write samples
/// let samples = vec![0.5f32; 256];
/// producer.push(&samples);
///
/// // Consumer thread: read samples
/// let mut output = vec![0.0f32; 256];
/// consumer.pop(&mut output);
/// ```
pub struct AudioRingBuffer;

impl AudioRingBuffer {
    /// Create a new ring buffer with the specified capacity and split into producer/consumer.
    ///
    /// # Arguments
    ///
    /// * `capacity_frames` - Maximum number of audio frames the buffer can hold
    /// * `channels` - Number of audio channels (samples per frame)
    ///
    /// # Returns
    ///
    /// A tuple of (producer, consumer) handles for the buffer.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(capacity_frames: u32, channels: u8) -> (AudioRingProducer, AudioRingConsumer) {
        let capacity_samples = capacity_frames as usize * channels as usize;
        let rb = HeapRb::<f32>::new(capacity_samples);
        let (prod, cons) = rb.split();
        let metrics = Arc::new(AudioMetrics::new());

        (
            AudioRingProducer {
                inner: prod,
                capacity_frames,
                channels,
                metrics: Arc::clone(&metrics),
            },
            AudioRingConsumer {
                inner: cons,
                capacity_frames,
                channels,
                metrics,
            },
        )
    }
}

impl AudioRingProducer {
    /// Push samples into the buffer.
    ///
    /// # Returns
    ///
    /// Number of samples actually written. May be less than input if buffer is full.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        let written = self.inner.push_slice(samples);
        let frames = written / self.channels as usize;
        self.metrics.record_received(frames as u64);

        if written < samples.len() {
            let dropped = (samples.len() - written) / self.channels as usize;
            self.metrics.record_dropped(dropped as u64);
            warn!(
                dropped_frames = dropped,
                "audio buffer overflow - frames dropped"
            );
        }

        written
    }

    /// Push samples, blocking until space is available or timeout.
    /// Returns the number of samples written.
    ///
    /// Note: This is NOT real-time safe. Use only in non-real-time contexts.
    pub fn push_blocking(&mut self, samples: &[f32], timeout_ms: u64) -> usize {
        use std::time::{Duration, Instant};
        
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut total_written = 0;

        while total_written < samples.len() && Instant::now() < deadline {
            let written = self.inner.push_slice(&samples[total_written..]);
            total_written += written;
            
            if written == 0 {
                std::thread::sleep(Duration::from_micros(100));
            }
        }

        let frames = total_written / self.channels as usize;
        self.metrics.record_received(frames as u64);
        total_written
    }

    /// Get current buffer occupancy.
    pub fn occupancy(&self) -> BufferOccupancy {
        let current_samples = self.inner.occupied_len();
        let current_frames = current_samples / self.channels as usize;
        BufferOccupancy::new(current_frames as u32, self.capacity_frames)
    }

    /// Check if the buffer has space for at least `frames` frames.
    #[inline]
    pub fn has_space_for(&self, frames: u32) -> bool {
        let samples_needed = frames as usize * self.channels as usize;
        self.inner.vacant_len() >= samples_needed
    }

    /// Get remaining capacity in frames.
    #[inline]
    pub fn available_frames(&self) -> u32 {
        (self.inner.vacant_len() / self.channels as usize) as u32
    }
}

impl AudioRingConsumer {
    /// Pop samples from the buffer.
    ///
    /// # Returns
    ///
    /// Number of samples actually read. May be less than buffer size if not enough data.
    pub fn pop(&mut self, output: &mut [f32]) -> usize {
        let read = self.inner.pop_slice(output);
        let frames = read / self.channels as usize;
        self.metrics.record_emitted(frames as u64);

        if read < output.len() && read == 0 {
            self.metrics.record_underrun();
            debug!("audio buffer underrun");
        }

        read
    }

    /// Pop samples, filling with silence if not enough data available.
    /// Always fills the entire output buffer.
    pub fn pop_or_silence(&mut self, output: &mut [f32]) -> usize {
        let read = self.inner.pop_slice(output);
        let frames = read / self.channels as usize;
        self.metrics.record_emitted(frames as u64);

        if read < output.len() {
            // Fill remaining with silence
            output[read..].fill(0.0);
            if read == 0 {
                self.metrics.record_underrun();
                debug!("audio buffer underrun - filled with silence");
            }
        }

        read
    }

    /// Get current buffer occupancy.
    pub fn occupancy(&self) -> BufferOccupancy {
        let current_samples = self.inner.occupied_len();
        let current_frames = current_samples / self.channels as usize;
        BufferOccupancy::new(current_frames as u32, self.capacity_frames)
    }

    /// Check if at least `frames` frames are available.
    #[inline]
    pub fn has_frames(&self, frames: u32) -> bool {
        let samples_needed = frames as usize * self.channels as usize;
        self.inner.occupied_len() >= samples_needed
    }

    /// Get number of available frames.
    #[inline]
    pub fn available_frames(&self) -> u32 {
        (self.inner.occupied_len() / self.channels as usize) as u32
    }

    /// Get a reference to the metrics.
    pub fn metrics(&self) -> &AudioMetrics {
        &self.metrics
    }
}

/// A bounded audio buffer with backpressure support.
///
/// Unlike [`AudioRingBuffer`], this buffer provides explicit backpressure
/// signaling for flow control in processing pipelines.
pub struct BoundedAudioBuffer {
    buffer: AudioBuffer,
    write_pos: usize,
    read_pos: usize,
    high_water_mark: usize,
    low_water_mark: usize,
}

impl BoundedAudioBuffer {
    /// Create a new bounded buffer.
    ///
    /// # Arguments
    ///
    /// * `capacity_frames` - Maximum frames the buffer can hold
    /// * `channels` - Number of audio channels
    /// * `high_water_mark` - Fill level (0.0-1.0) to signal backpressure
    /// * `low_water_mark` - Fill level (0.0-1.0) to release backpressure
    pub fn new(
        capacity_frames: usize,
        channels: u8,
        high_water_mark: f32,
        low_water_mark: f32,
    ) -> Self {
        let capacity = capacity_frames * channels as usize;
        Self {
            buffer: AudioBuffer::new(capacity_frames, channels),
            write_pos: 0,
            read_pos: 0,
            high_water_mark: (capacity as f32 * high_water_mark) as usize,
            low_water_mark: (capacity as f32 * low_water_mark) as usize,
        }
    }

    /// Check if backpressure should be applied (buffer is getting full).
    pub fn should_backpressure(&self) -> bool {
        self.len() >= self.high_water_mark
    }

    /// Check if backpressure can be released (buffer has drained).
    pub fn can_release_backpressure(&self) -> bool {
        self.len() <= self.low_water_mark
    }

    /// Current number of samples in the buffer.
    #[inline]
    pub fn len(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            self.buffer.data.len() - self.read_pos + self.write_pos
        }
    }

    /// Check if buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.write_pos == self.read_pos
    }

    /// Get buffer capacity in samples.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.buffer.data.len()
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.write_pos = 0;
        self.read_pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_buffer_basic() {
        let buf = AudioBuffer::new(1024, 2);
        assert_eq!(buf.frames(), 1024);
        assert_eq!(buf.channels(), 2);
        assert_eq!(buf.samples().len(), 2048);
    }

    #[test]
    fn test_ring_buffer_push_pop() {
        let (mut prod, mut cons) = AudioRingBuffer::new(1024, 2);

        let input: Vec<f32> = (0..512).map(|i| i as f32 / 512.0).collect();
        let written = prod.push(&input);
        assert_eq!(written, 512);

        let mut output = vec![0.0f32; 512];
        let read = cons.pop(&mut output);
        assert_eq!(read, 512);
        assert_eq!(input, output);
    }

    #[test]
    fn test_ring_buffer_overflow() {
        let (mut prod, _cons) = AudioRingBuffer::new(64, 1);

        let input = vec![1.0f32; 128];
        let written = prod.push(&input);
        assert!(written < 128); // Should overflow
    }

    #[test]
    fn test_ring_buffer_underrun() {
        let (_prod, mut cons) = AudioRingBuffer::new(64, 1);

        let mut output = vec![0.0f32; 64];
        let read = cons.pop(&mut output);
        assert_eq!(read, 0); // Underrun
    }

    #[test]
    fn test_ring_buffer_pop_or_silence() {
        let (mut prod, mut cons) = AudioRingBuffer::new(64, 1);

        prod.push(&[1.0, 2.0, 3.0, 4.0]);

        let mut output = vec![0.0f32; 8];
        cons.pop_or_silence(&mut output);

        assert_eq!(output[0..4], [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(output[4..8], [0.0, 0.0, 0.0, 0.0]); // Silence
    }

    #[test]
    fn test_bounded_buffer_backpressure() {
        let buf = BoundedAudioBuffer::new(100, 1, 0.8, 0.2);
        assert!(!buf.should_backpressure());
        assert!(buf.can_release_backpressure());
        assert!(buf.is_empty());
    }
}
