//! Metrics definitions for OpenSpeechBridge.
//!
//! This module defines the metrics collected throughout the runtime.
//! Metrics are designed for observability without compromising privacy.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Audio pipeline metrics.
#[derive(Debug, Default)]
pub struct AudioMetrics {
    /// Total frames received from capture
    pub frames_received: AtomicU64,
    /// Total frames emitted to playback
    pub frames_emitted: AtomicU64,
    /// Frames dropped due to buffer overflow
    pub frames_dropped: AtomicU64,
    /// Buffer underrun events
    pub underruns: AtomicU64,
    /// Buffer overrun events
    pub overruns: AtomicU64,
}

impl AudioMetrics {
    /// Create new audio metrics.
    pub const fn new() -> Self {
        Self {
            frames_received: AtomicU64::new(0),
            frames_emitted: AtomicU64::new(0),
            frames_dropped: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            overruns: AtomicU64::new(0),
        }
    }

    /// Record frames received.
    #[inline]
    pub fn record_received(&self, frames: u64) {
        self.frames_received.fetch_add(frames, Ordering::Relaxed);
    }

    /// Record frames emitted.
    #[inline]
    pub fn record_emitted(&self, frames: u64) {
        self.frames_emitted.fetch_add(frames, Ordering::Relaxed);
    }

    /// Record dropped frames.
    #[inline]
    pub fn record_dropped(&self, frames: u64) {
        self.frames_dropped.fetch_add(frames, Ordering::Relaxed);
        self.overruns.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an underrun event.
    #[inline]
    pub fn record_underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }

    /// Get a snapshot of current metrics.
    pub fn snapshot(&self) -> AudioMetricsSnapshot {
        AudioMetricsSnapshot {
            frames_received: self.frames_received.load(Ordering::Relaxed),
            frames_emitted: self.frames_emitted.load(Ordering::Relaxed),
            frames_dropped: self.frames_dropped.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of audio metrics at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioMetricsSnapshot {
    pub frames_received: u64,
    pub frames_emitted: u64,
    pub frames_dropped: u64,
    pub underruns: u64,
    pub overruns: u64,
}

/// Latency measurement.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LatencyStats {
    /// Minimum latency in microseconds
    pub min_us: u64,
    /// Maximum latency in microseconds
    pub max_us: u64,
    /// Average latency in microseconds
    pub avg_us: u64,
    /// 95th percentile latency in microseconds
    pub p95_us: u64,
    /// 99th percentile latency in microseconds
    pub p99_us: u64,
    /// Number of samples
    pub count: u64,
}

/// Buffer occupancy measurement.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BufferOccupancy {
    /// Current fill level (0.0 - 1.0)
    pub fill_ratio: f32,
    /// Capacity in frames
    pub capacity_frames: u32,
    /// Current frames in buffer
    pub current_frames: u32,
}

impl BufferOccupancy {
    /// Create a new occupancy measurement.
    pub fn new(current_frames: u32, capacity_frames: u32) -> Self {
        let fill_ratio = if capacity_frames > 0 {
            current_frames as f32 / capacity_frames as f32
        } else {
            0.0
        };
        Self {
            fill_ratio,
            capacity_frames,
            current_frames,
        }
    }

    /// Check if buffer is critically low (< 10%)
    pub fn is_low(&self) -> bool {
        self.fill_ratio < 0.1
    }

    /// Check if buffer is critically high (> 90%)
    pub fn is_high(&self) -> bool {
        self.fill_ratio > 0.9
    }
}

/// Simple latency tracker for measuring processing time.
#[derive(Debug)]
pub struct LatencyTracker {
    start: Option<Instant>,
    samples: Vec<u64>,
    max_samples: usize,
}

impl LatencyTracker {
    /// Create a new latency tracker.
    pub fn new(max_samples: usize) -> Self {
        Self {
            start: None,
            samples: Vec::with_capacity(max_samples),
            max_samples,
        }
    }

    /// Start timing.
    #[inline]
    pub fn start(&mut self) {
        self.start = Some(Instant::now());
    }

    /// Stop timing and record the sample.
    #[inline]
    pub fn stop(&mut self) {
        if let Some(start) = self.start.take() {
            let elapsed_us = start.elapsed().as_micros() as u64;
            if self.samples.len() >= self.max_samples {
                self.samples.remove(0);
            }
            self.samples.push(elapsed_us);
        }
    }

    /// Get current latency statistics.
    pub fn stats(&self) -> Option<LatencyStats> {
        if self.samples.is_empty() {
            return None;
        }

        let mut sorted = self.samples.clone();
        sorted.sort_unstable();

        let count = sorted.len() as u64;
        let min_us = sorted[0];
        let max_us = sorted[sorted.len() - 1];
        let avg_us = sorted.iter().sum::<u64>() / count;
        let p95_idx = (sorted.len() as f64 * 0.95) as usize;
        let p99_idx = (sorted.len() as f64 * 0.99) as usize;
        let p95_us = sorted[p95_idx.min(sorted.len() - 1)];
        let p99_us = sorted[p99_idx.min(sorted.len() - 1)];

        Some(LatencyStats {
            min_us,
            max_us,
            avg_us,
            p95_us,
            p99_us,
            count,
        })
    }

    /// Clear all samples.
    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_metrics() {
        let metrics = AudioMetrics::new();
        metrics.record_received(100);
        metrics.record_emitted(95);
        metrics.record_dropped(5);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.frames_received, 100);
        assert_eq!(snapshot.frames_emitted, 95);
        assert_eq!(snapshot.frames_dropped, 5);
        assert_eq!(snapshot.overruns, 1);
    }

    #[test]
    fn test_audio_metrics_underrun() {
        let metrics = AudioMetrics::new();
        metrics.record_underrun();
        metrics.record_underrun();
        metrics.record_underrun();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.underruns, 3);
    }

    #[test]
    fn test_audio_metrics_multiple_operations() {
        let metrics = AudioMetrics::new();

        for _ in 0..100 {
            metrics.record_received(10);
            metrics.record_emitted(10);
        }
        metrics.record_dropped(50);
        metrics.record_dropped(30);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.frames_received, 1000);
        assert_eq!(snapshot.frames_emitted, 1000);
        assert_eq!(snapshot.frames_dropped, 80);
        assert_eq!(snapshot.overruns, 2);
    }

    #[test]
    fn test_buffer_occupancy() {
        let occ = BufferOccupancy::new(50, 100);
        assert!((occ.fill_ratio - 0.5).abs() < 0.001);
        assert!(!occ.is_low());
        assert!(!occ.is_high());

        let low = BufferOccupancy::new(5, 100);
        assert!(low.is_low());

        let high = BufferOccupancy::new(95, 100);
        assert!(high.is_high());
    }

    #[test]
    fn test_buffer_occupancy_empty() {
        let occ = BufferOccupancy::new(0, 100);
        assert_eq!(occ.fill_ratio, 0.0);
        assert!(occ.is_low());
        assert!(!occ.is_high());
    }

    #[test]
    fn test_buffer_occupancy_full() {
        let occ = BufferOccupancy::new(100, 100);
        assert!((occ.fill_ratio - 1.0).abs() < 0.001);
        assert!(!occ.is_low());
        assert!(occ.is_high());
    }

    #[test]
    fn test_buffer_occupancy_zero_capacity() {
        let occ = BufferOccupancy::new(0, 0);
        assert_eq!(occ.fill_ratio, 0.0);
    }

    #[test]
    fn test_latency_tracker() {
        let mut tracker = LatencyTracker::new(100);

        for _ in 0..10 {
            tracker.start();
            std::thread::sleep(std::time::Duration::from_micros(100));
            tracker.stop();
        }

        let stats = tracker.stats().unwrap();
        assert_eq!(stats.count, 10);
        assert!(stats.min_us >= 100);
        assert!(stats.avg_us >= 100);
    }

    #[test]
    fn test_latency_tracker_empty() {
        let tracker = LatencyTracker::new(100);
        assert!(tracker.stats().is_none());
    }

    #[test]
    fn test_latency_tracker_single_sample() {
        let mut tracker = LatencyTracker::new(100);
        tracker.start();
        std::thread::sleep(std::time::Duration::from_micros(50));
        tracker.stop();

        let stats = tracker.stats().unwrap();
        assert_eq!(stats.count, 1);
        assert_eq!(stats.min_us, stats.max_us);
        assert_eq!(stats.avg_us, stats.min_us);
        assert_eq!(stats.p95_us, stats.min_us);
        assert_eq!(stats.p99_us, stats.min_us);
    }

    #[test]
    fn test_latency_tracker_clear() {
        let mut tracker = LatencyTracker::new(100);
        tracker.start();
        tracker.stop();

        assert!(tracker.stats().is_some());
        tracker.clear();
        assert!(tracker.stats().is_none());
    }

    #[test]
    fn test_latency_tracker_max_samples() {
        let mut tracker = LatencyTracker::new(5);

        for _ in 0..10 {
            tracker.start();
            tracker.stop();
        }

        let stats = tracker.stats().unwrap();
        assert_eq!(stats.count, 5); // Should only keep last 5
    }

    #[test]
    fn test_latency_tracker_stop_without_start() {
        let mut tracker = LatencyTracker::new(100);
        tracker.stop(); // Should not panic
        assert!(tracker.stats().is_none());
    }

    #[test]
    fn test_latency_stats_percentiles() {
        let mut tracker = LatencyTracker::new(100);

        // Add samples with increasing latency
        for _ in 1..=100 {
            tracker.start();
            // Simulate by directly adding samples (we can't easily control timing)
            tracker.stop();
        }

        let stats = tracker.stats().unwrap();
        assert!(stats.p95_us <= stats.p99_us);
        assert!(stats.p99_us <= stats.max_us);
        assert!(stats.min_us <= stats.avg_us);
        assert!(stats.avg_us <= stats.max_us);
    }
}
