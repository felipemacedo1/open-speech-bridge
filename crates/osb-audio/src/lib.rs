//! # osb-audio
//!
//! Audio buffer management and processing for OpenSpeechBridge.
//!
//! This crate provides:
//! - Lock-free ring buffers for real-time audio
//! - Sample format conversion
//! - Resampling utilities (fixed-frame and arbitrary-length streaming)
//! - Bounded Voice Activity Detection (VAD) and endpoint segmentation
//!
//! ## Design Principles
//!
//! - Real-time safe: No allocations or blocking in hot paths
//! - Lock-free: SPSC ring buffers for producer/consumer patterns
//! - Bounded: All buffers have fixed capacity to prevent unbounded growth
//! - Observable: Metrics for buffer occupancy, drops, and underruns

pub mod buffer;
pub mod convert;
pub mod resample;
pub mod vad;

pub use buffer::{
    AudioBuffer, AudioRingBuffer, AudioRingConsumer, AudioRingProducer, BoundedAudioBuffer,
};
pub use convert::{deinterleave, interleave, SampleConverter};
pub use resample::{resample_mono_buffer, Resampler, StreamingMonoResampler};
pub use vad::{UtteranceSegment, VadConfig, VadSegmenter, VadState};
