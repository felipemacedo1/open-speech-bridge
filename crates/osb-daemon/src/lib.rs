//! # osb-daemon
//!
//! Background daemon for OpenSpeechBridge runtime.
//!
//! The daemon manages:
//! - Audio capture and playback streams
//! - Virtual audio devices
//! - Engine lifecycle
//! - Processing pipeline
//! - Metrics collection
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    osb-daemon                           │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
//! │  │   Audio     │  │  Pipeline   │  │   Engine    │     │
//! │  │  Manager    │  │  Manager    │  │  Supervisor │     │
//! │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘     │
//! │         │                │                │             │
//! │         └────────────────┼────────────────┘             │
//! │                          │                              │
//! │                   ┌──────┴──────┐                       │
//! │                   │   Runtime   │                       │
//! │                   └─────────────┘                       │
//! └─────────────────────────────────────────────────────────┘
//! ```

pub mod audio_manager;
pub mod engine_supervisor;
pub mod pipeline;
pub mod runtime;

pub use runtime::{Runtime, RuntimeConfig, RuntimeState};
