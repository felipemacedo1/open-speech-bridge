//! # osb-audio
//!
//! Audio buffer management and processing for OpenSpeechBridge.
//!
//! This crate provides:
//! - Lock-free ring buffers for real-time audio
//! - Sample format conversion
//! - Resampling utilities
//! - Buffer management with backpressure
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

pub use buffer::{AudioBuffer, AudioRingBuffer, BoundedAudioBuffer};
pub use convert::{deinterleave, interleave, SampleConverter};
pub use resample::Resampler;
