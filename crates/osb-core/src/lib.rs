//! # osb-core
//!
//! Core types, errors, configuration, and utilities for OpenSpeechBridge.
//!
//! This crate provides the foundational abstractions used across the runtime:
//! - Audio format specifications
//! - Error types
//! - Configuration structures
//! - Common utilities
//!
//! ## Design Principles
//!
//! - No I/O operations - pure data types and transformations
//! - No platform-specific code
//! - Minimal dependencies
//! - `no_std` compatible where practical

pub mod audio;
pub mod config;
pub mod error;
pub mod metrics;

pub use audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
pub use config::Config;
pub use error::{Error, Result};
