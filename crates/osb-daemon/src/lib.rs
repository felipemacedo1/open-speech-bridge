//! # osb-daemon
//!
//! Background daemon for OpenSpeechBridge runtime.
//!
//! The daemon manages:
//! - Audio capture and playback streams
//! - Virtual audio devices
//! - Engine lifecycle and Unix Domain Socket worker supervision
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
pub mod ipc;
pub mod pipeline;
pub mod runtime;
pub mod worker;

pub use engine_supervisor::{
    EngineHandler, EngineMetricsSnapshot, EngineSupervisor, RestartPolicy,
};
pub use ipc::{EngineRpcClient, IpcClientConfig};
pub use pipeline::{
    split_text_into_tts_clauses, Pipeline, PipelineConfig, PipelineMetricsSnapshot, PipelineRoute,
    PipelineStage, TranslatedUtterance,
};
pub use runtime::{Runtime, RuntimeConfig, RuntimeState};
pub use worker::{ManagedWorker, WorkerSpec, WorkerTransport};
