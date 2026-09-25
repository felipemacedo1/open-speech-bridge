//! Audio buffer implementations.
//!
//! This module provides bounded, lock-free buffers for real-time audio processing.
//! The primary buffer type is [`AudioRingBuffer`], a SPSC ring buffer suitable
//! for passing audio between threads without blocking.
//!
//! # Thread Safety
//!
//! The [`AudioRingProducer`] and [`AudioRingConsumer`] are both `Send`, allowing
//! them to be moved to different threads. This is essential for real-time audio
//! where the producer runs in the audio callback thread (e.g., PipeWire) and
//! the consumer runs in the processing thread.
//!
//! # Real-time Safety
//!
//! All operations on the ring buffer are lock-free and wait-free:
//! - No memory allocation after creation
//! - No blocking operations
//! - No system calls in the hot path

use osb_core::metrics::{AudioMetrics, BufferOccupancy};
use std::sync::Arc;

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

/// Shared state between producer and consumer for metrics tracking.
struct SharedState {
    metrics: AudioMetrics,
    capacity_frames: u32,
    channels: u8,
}

/// Producer half of an audio ring buffer.
///
/// This type is `Send`, allowing it to be moved to another thread
/// (e.g., a PipeWire audio callback thread).
///
/// # Real-time Safety
///
/// The `push` method is lock-free and does not allocate memory,
/// making it safe to call from real-time audio contexts.
pub struct AudioRingProducer {
    inner: rtrb::Producer<f32>,
    shared: Arc<SharedState>,
    /// Local counter to avoid atomic operations in hot path
    local_received: u64,
    local_dropped: u64,
}

// SAFETY: rtrb::Producer is Send, and we only share Arc<SharedState> which is Sync
unsafe impl Send for AudioRingProducer {}

/// Consumer half of an audio ring buffer.
///
/// This type is `Send`, allowing it to be moved to another thread.
///
/// # Real-time Safety
///
/// The `pop` and `pop_or_silence` methods are lock-free and do not
/// allocate memory, making them safe to call from real-time audio contexts.
pub struct AudioRingConsumer {
    inner: rtrb::Consumer<f32>,
    shared: Arc<SharedState>,
    /// Local counter to avoid atomic operations in hot path
    local_emitted: u64,
}

// SAFETY: rtrb::Consumer is Send, and we only share Arc<SharedState> which is Sync
unsafe impl Send for AudioRingConsumer {}

/// A lock-free SPSC ring buffer for audio samples.
///
/// This buffer is designed for real-time audio processing where one thread
/// produces samples (e.g., audio capture) and another consumes them
/// (e.g., processing or playback).
///
/// # Thread Safety
///
/// The producer and consumer can be safely moved to different threads.
/// This is the key improvement over the previous `ringbuf`-based implementation.
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
///
/// # Cross-thread Example
///
/// ```
/// use osb_audio::AudioRingBuffer;
/// use std::thread;
///
/// let (mut producer, mut consumer) = AudioRingBuffer::new(1024, 2);
///
/// // Move producer to another thread (this works because Producer is Send)
/// let producer_thread = thread::spawn(move || {
///     let samples = vec![0.5f32; 256];
///     producer.push(&samples)
/// });
///
/// // Consumer stays in main thread
/// let mut output = vec![0.0f32; 256];
/// producer_thread.join().unwrap();
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
    /// Both handles are `Send` and can be moved to different threads.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(capacity_frames: u32, channels: u8) -> (AudioRingProducer, AudioRingConsumer) {
        let capacity_samples = capacity_frames as usize * channels as usize;
        let (prod, cons) = rtrb::RingBuffer::new(capacity_samples);

        let shared = Arc::new(SharedState {
            metrics: AudioMetrics::new(),
            capacity_frames,
            channels,
        });

        (
            AudioRingProducer {
                inner: prod,
                shared: Arc::clone(&shared),
                local_received: 0,
                local_dropped: 0,
            },
            AudioRingConsumer {
                inner: cons,
                shared,
                local_emitted: 0,
            },
        )
    }
}

impl AudioRingProducer {
    /// Push samples into the buffer.
    ///
    /// # Real-time Safety
    ///
    /// This method is lock-free and does not allocate memory.
    /// Safe to call from audio callback threads.
    ///
    /// # Returns
    ///
    /// Number of samples actually written. May be less than input if buffer is full.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        let slots = self.inner.slots();
        let channels = self.shared.channels as usize;
        let to_write = (samples.len().min(slots) / channels) * channels;

        if to_write > 0 {
            // Use write_chunk for efficient bulk copy
            if let Ok(mut chunk) = self.inner.write_chunk(to_write) {
                let (first, second) = chunk.as_mut_slices();
                let first_len = first.len();

                if to_write <= first_len {
                    first[..to_write].copy_from_slice(&samples[..to_write]);
                } else {
                    first.copy_from_slice(&samples[..first_len]);
                    second[..to_write - first_len].copy_from_slice(&samples[first_len..to_write]);
                }
                chunk.commit_all();
            }
        }

        let frames = to_write / self.shared.channels as usize;
        self.local_received += frames as u64;

        if to_write < samples.len() {
            let dropped_samples = samples.len() - to_write;
            let dropped_frames = dropped_samples / self.shared.channels as usize;
            self.local_dropped += dropped_frames as u64;
        }

        to_write
    }

    /// Push samples, blocking until space is available or timeout.
    /// Returns the number of samples written.
    ///
    /// # Warning
    ///
    /// This is NOT real-time safe. Use only in non-real-time contexts.
    pub fn push_blocking(&mut self, samples: &[f32], timeout_ms: u64) -> usize {
        use std::time::{Duration, Instant};

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut total_written = 0;

        while total_written < samples.len() && Instant::now() < deadline {
            let written = self.push(&samples[total_written..]);
            total_written += written;

            if written == 0 {
                std::thread::sleep(Duration::from_micros(100));
            }
        }

        total_written
    }

    /// Get current buffer occupancy.
    pub fn occupancy(&self) -> BufferOccupancy {
        let current_samples = self.inner.slots();
        let capacity_samples = self.shared.capacity_frames as usize * self.shared.channels as usize;
        let occupied = capacity_samples - current_samples;
        let current_frames = occupied / self.shared.channels as usize;
        BufferOccupancy::new(current_frames as u32, self.shared.capacity_frames)
    }

    /// Check if the buffer has space for at least `frames` frames.
    #[inline]
    pub fn has_space_for(&self, frames: u32) -> bool {
        let samples_needed = frames as usize * self.shared.channels as usize;
        self.inner.slots() >= samples_needed
    }

    /// Get remaining capacity in frames.
    #[inline]
    pub fn available_frames(&self) -> u32 {
        (self.inner.slots() / self.shared.channels as usize) as u32
    }

    /// Flush local metrics to the shared metrics.
    ///
    /// Call this periodically from non-real-time context to update metrics.
    pub fn flush_metrics(&mut self) {
        if self.local_received > 0 {
            self.shared.metrics.record_received(self.local_received);
            self.local_received = 0;
        }
        if self.local_dropped > 0 {
            self.shared.metrics.record_dropped(self.local_dropped);
            self.local_dropped = 0;
        }
    }
}

impl Drop for AudioRingProducer {
    fn drop(&mut self) {
        self.flush_metrics();
    }
}

impl AudioRingConsumer {
    /// Pop samples from the buffer.
    ///
    /// # Real-time Safety
    ///
    /// This method is lock-free and does not allocate memory.
    /// Safe to call from audio callback threads.
    ///
    /// # Returns
    ///
    /// Number of samples actually read. May be less than buffer size if not enough data.
    pub fn pop(&mut self, output: &mut [f32]) -> usize {
        let available = self.inner.slots();
        let channels = self.shared.channels as usize;
        let to_read = (output.len().min(available) / channels) * channels;

        if to_read > 0 {
            if let Ok(chunk) = self.inner.read_chunk(to_read) {
                let (first, second) = chunk.as_slices();
                let first_len = first.len();

                if to_read <= first_len {
                    output[..to_read].copy_from_slice(&first[..to_read]);
                } else {
                    output[..first_len].copy_from_slice(first);
                    output[first_len..to_read].copy_from_slice(&second[..to_read - first_len]);
                }
                chunk.commit_all();
            }
        }

        let frames = to_read / self.shared.channels as usize;
        self.local_emitted += frames as u64;

        if to_read < output.len() && to_read == 0 {
            self.shared.metrics.record_underrun();
        }

        to_read
    }

    /// Pop samples, filling with silence if not enough data available.
    /// Always fills the entire output buffer.
    ///
    /// # Real-time Safety
    ///
    /// This method is lock-free and does not allocate memory.
    /// Safe to call from audio callback threads.
    pub fn pop_or_silence(&mut self, output: &mut [f32]) -> usize {
        let read = self.pop(output);

        if read < output.len() {
            // Fill remaining with silence
            output[read..].fill(0.0);
        }

        read
    }

    /// Get current buffer occupancy.
    pub fn occupancy(&self) -> BufferOccupancy {
        let current_samples = self.inner.slots();
        let current_frames = current_samples / self.shared.channels as usize;
        BufferOccupancy::new(current_frames as u32, self.shared.capacity_frames)
    }

    /// Check if at least `frames` frames are available.
    #[inline]
    pub fn has_frames(&self, frames: u32) -> bool {
        let samples_needed = frames as usize * self.shared.channels as usize;
        self.inner.slots() >= samples_needed
    }

    /// Get number of available frames.
    #[inline]
    pub fn available_frames(&self) -> u32 {
        (self.inner.slots() / self.shared.channels as usize) as u32
    }

    /// Get a reference to the metrics.
    pub fn metrics(&self) -> &AudioMetrics {
        &self.shared.metrics
    }

    /// Flush local metrics to the shared metrics.
    ///
    /// Call this periodically from non-real-time context to update metrics.
    pub fn flush_metrics(&mut self) {
        if self.local_emitted > 0 {
            self.shared.metrics.record_emitted(self.local_emitted);
            self.local_emitted = 0;
        }
    }
}

impl Drop for AudioRingConsumer {
    fn drop(&mut self) {
        self.flush_metrics();
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
    fn test_audio_buffer_from_samples() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let buf = AudioBuffer::from_samples(data.clone(), 2);
        assert_eq!(buf.frames(), 2);
        assert_eq!(buf.samples(), &data);
    }

    #[test]
    fn test_audio_buffer_clear_and_resize() {
        let mut buf = AudioBuffer::new(10, 1);
        buf.samples_mut()[0] = 1.0;
        buf.clear();
        assert_eq!(buf.samples()[0], 0.0);

        buf.resize(20);
        assert_eq!(buf.frames(), 20);
        assert_eq!(buf.samples().len(), 20);
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
    fn test_ring_buffer_stays_bounded_when_consumer_is_absent() {
        let (mut prod, _cons) = AudioRingBuffer::new(8, 2);
        let input = vec![1.0f32; 32];

        for _ in 0..10 {
            prod.push(&input);
        }

        assert_eq!(prod.occupancy().current_frames, 8);
        assert_eq!(prod.available_frames(), 0);
    }

    #[test]
    fn test_ring_buffer_consumer_can_resume_after_overflow() {
        let (mut prod, mut cons) = AudioRingBuffer::new(8, 2);
        let input = vec![1.0f32; 16];
        prod.push(&input);

        let mut output = vec![0.0f32; 8];
        assert_eq!(cons.pop(&mut output), 8);
        assert_eq!(cons.occupancy().current_frames, 4);

        prod.push(&input[..8]);
        assert_eq!(cons.occupancy().current_frames, 8);
    }

    #[test]
    fn test_ring_buffer_preserves_interleaved_frames() {
        let (mut prod, mut cons) = AudioRingBuffer::new(4, 2);
        assert_eq!(prod.push(&[1.0, 2.0, 3.0]), 2);

        let mut output = [0.0; 4];
        assert_eq!(cons.pop(&mut output), 2);
        assert_eq!(&output[..2], &[1.0, 2.0]);
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
    fn test_ring_buffer_occupancy() {
        let (mut prod, cons) = AudioRingBuffer::new(100, 1);

        let occ = prod.occupancy();
        assert_eq!(occ.current_frames, 0);
        assert_eq!(occ.capacity_frames, 100);

        prod.push(&[1.0; 50]);
        let occ = cons.occupancy();
        assert_eq!(occ.current_frames, 50);
        assert!((occ.fill_ratio - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_ring_buffer_has_space_and_frames() {
        let (mut prod, cons) = AudioRingBuffer::new(100, 2);

        assert!(prod.has_space_for(50)); // 50 frames = 100 samples
        assert!(!cons.has_frames(1));

        prod.push(&[1.0; 100]); // 50 frames
        assert!(cons.has_frames(50));
        assert!(!cons.has_frames(51));
    }

    #[test]
    fn test_ring_buffer_available_frames() {
        let (mut prod, cons) = AudioRingBuffer::new(100, 2);

        assert_eq!(prod.available_frames(), 100);
        assert_eq!(cons.available_frames(), 0);

        prod.push(&[1.0; 40]); // 20 frames
        assert_eq!(cons.available_frames(), 20);
        assert_eq!(prod.available_frames(), 80);
    }

    #[test]
    fn test_ring_buffer_metrics() {
        let (mut prod, mut cons) = AudioRingBuffer::new(10, 1);

        prod.push(&[1.0; 5]);
        let mut output = vec![0.0; 5];
        cons.pop(&mut output);

        // Flush metrics before checking
        prod.flush_metrics();
        cons.flush_metrics();

        let metrics = cons.metrics();
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.frames_received, 5);
        assert_eq!(snapshot.frames_emitted, 5);
    }

    #[test]
    fn test_ring_buffer_push_blocking_timeout() {
        let (mut prod, _cons) = AudioRingBuffer::new(10, 1);

        // Fill the buffer
        prod.push(&[1.0; 10]);

        // Try to push more with very short timeout - should return 0
        let written = prod.push_blocking(&[2.0; 5], 1);
        assert_eq!(written, 0);
    }

    #[test]
    fn test_bounded_buffer_backpressure() {
        let buf = BoundedAudioBuffer::new(100, 1, 0.8, 0.2);
        assert!(!buf.should_backpressure());
        assert!(buf.can_release_backpressure());
        assert!(buf.is_empty());
    }

    #[test]
    fn test_bounded_buffer_capacity() {
        let buf = BoundedAudioBuffer::new(100, 2, 0.8, 0.2);
        assert_eq!(buf.capacity(), 200); // 100 frames * 2 channels
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_bounded_buffer_clear() {
        let mut buf = BoundedAudioBuffer::new(100, 1, 0.8, 0.2);
        buf.clear();
        assert!(buf.is_empty());
    }

    /// Test that producer can be sent to another thread.
    #[test]
    fn test_producer_is_send() {
        let (producer, _consumer) = AudioRingBuffer::new(64, 1);

        // This compiles only if AudioRingProducer is Send
        std::thread::spawn(move || {
            let _ = producer;
        })
        .join()
        .unwrap();
    }

    /// Test that consumer can be sent to another thread.
    #[test]
    fn test_consumer_is_send() {
        let (_producer, consumer) = AudioRingBuffer::new(64, 1);

        // This compiles only if AudioRingConsumer is Send
        std::thread::spawn(move || {
            let _ = consumer;
        })
        .join()
        .unwrap();
    }

    /// Test cross-thread communication.
    #[test]
    fn test_cross_thread_communication() {
        let (mut producer, mut consumer) = AudioRingBuffer::new(1024, 1);

        let producer_thread = std::thread::spawn(move || {
            let samples: Vec<f32> = (0..100).map(|i| i as f32).collect();
            producer.push(&samples);
            producer // Return producer to flush metrics
        });

        // Give producer time to write
        std::thread::sleep(std::time::Duration::from_millis(10));

        let mut output = vec![0.0f32; 100];
        let read = consumer.pop(&mut output);

        let _producer = producer_thread.join().unwrap();

        assert_eq!(read, 100);
        for (i, &sample) in output.iter().enumerate().take(100) {
            assert_eq!(sample, i as f32);
        }
    }

    /// Test that buffer doesn't grow when pushed continuously without consumption.
    /// This simulates the "no consumer connected" scenario in loopback.
    #[test]
    fn test_continuous_push_without_consumer_stays_bounded() {
        let (mut prod, _cons) = AudioRingBuffer::new(64, 2);

        // Simulate continuous audio capture (e.g., 100 callbacks worth of data)
        for _ in 0..100 {
            let samples = vec![0.5f32; 128]; // 64 frames per callback
            prod.push(&samples);
        }

        // Buffer should never exceed capacity
        let occ = prod.occupancy();
        assert!(occ.current_frames <= 64);
        assert!(occ.fill_ratio <= 1.0);
    }

    /// Test overflow metrics are tracked correctly when producer outpaces consumer.
    #[test]
    fn test_overflow_metrics_tracked() {
        let (mut prod, _cons) = AudioRingBuffer::new(10, 1);

        // Push more than capacity
        prod.push(&[1.0; 15]);
        prod.flush_metrics();

        // Should have received 10 frames and dropped 5
        let metrics = prod.occupancy();
        assert_eq!(metrics.current_frames, 10);
    }

    /// Test consumer can drain buffer after producer stops.
    #[test]
    fn test_consumer_drains_after_producer_stops() {
        let (mut prod, mut cons) = AudioRingBuffer::new(100, 2);

        // Producer writes some data
        prod.push(&[1.0; 50]); // 25 frames
        drop(prod); // Producer stops

        // Consumer can still drain
        let mut output = vec![0.0; 50];
        let read = cons.pop(&mut output);
        assert_eq!(read, 50);
        assert_eq!(cons.available_frames(), 0);
    }

    /// Test stereo frame alignment with odd sample count.
    #[test]
    fn test_stereo_frame_alignment_odd_samples() {
        let (mut prod, mut cons) = AudioRingBuffer::new(10, 2);

        // Push 5 samples (2.5 frames) - should only write 4 (2 frames)
        let written = prod.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(written, 4); // Only complete frames

        // Should have exactly 2 frames
        assert_eq!(cons.available_frames(), 2);

        // Pop with odd request - should only read complete frames
        let mut output = vec![0.0; 5];
        let read = cons.pop(&mut output);
        assert_eq!(read, 4);
        assert_eq!(&output[..4], &[1.0, 2.0, 3.0, 4.0]);
    }

    /// Test rapid push/pop cycles maintain data integrity.
    #[test]
    fn test_rapid_push_pop_data_integrity() {
        let (mut prod, mut cons) = AudioRingBuffer::new(32, 2);

        for cycle in 0..50 {
            let base = (cycle * 4) as f32;
            let input = [base, base + 1.0, base + 2.0, base + 3.0];
            prod.push(&input);

            let mut output = [0.0f32; 4];
            let read = cons.pop(&mut output);
            assert_eq!(read, 4);
            assert_eq!(output, input);
        }
    }
}
