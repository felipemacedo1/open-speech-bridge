//! # osb-protocol
//!
//! Engine protocol and capability definitions for OpenSpeechBridge.
//!
//! This crate defines the contract between the runtime and speech processing engines.
//! Engines can be implemented in Rust or Python and communicate via this protocol.
//!
//! ## Design Principles
//!
//! - **Engine Independence**: The runtime does not depend on any specific ML framework
//! - **Capability Negotiation**: Engines advertise what they support
//! - **Versioned Protocol**: Changes are backwards compatible where possible
//! - **Streaming First**: Designed for incremental processing

pub mod capability;
pub mod engine;
pub mod message;

pub use capability::{Capability, CapabilitySet, LanguagePair};
pub use engine::{EngineId, EngineInfo, EngineState, EngineType};
pub use message::{EngineRequest, EngineResponse, ProcessingResult};

/// Protocol version.
pub const PROTOCOL_VERSION: &str = "0.1.0";
