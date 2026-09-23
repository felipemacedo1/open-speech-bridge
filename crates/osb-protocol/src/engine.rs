//! Engine identification and state management.

use crate::capability::CapabilitySet;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for an engine instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EngineId(pub String);

impl EngineId {
    /// Create a new engine ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EngineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for EngineId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for EngineId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Type of engine implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineType {
    /// Native Rust engine (in-process)
    Native,
    /// Python engine (separate process)
    Python,
    /// External process engine
    External,
    /// Mock engine for testing
    Mock,
}

impl fmt::Display for EngineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native => write!(f, "native"),
            Self::Python => write!(f, "python"),
            Self::External => write!(f, "external"),
            Self::Mock => write!(f, "mock"),
        }
    }
}

/// Current state of an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    /// Engine is not started
    Stopped,
    /// Engine is starting up
    Starting,
    /// Engine is ready to process
    Ready,
    /// Engine is currently processing
    Processing,
    /// Engine is paused
    Paused,
    /// Engine encountered an error
    Error,
    /// Engine is shutting down
    ShuttingDown,
}

impl EngineState {
    /// Check if the engine is available for processing.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Check if the engine is in an error state.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }

    /// Check if the engine is stopped.
    pub fn is_stopped(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}

impl fmt::Display for EngineState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stopped => write!(f, "stopped"),
            Self::Starting => write!(f, "starting"),
            Self::Ready => write!(f, "ready"),
            Self::Processing => write!(f, "processing"),
            Self::Paused => write!(f, "paused"),
            Self::Error => write!(f, "error"),
            Self::ShuttingDown => write!(f, "shutting_down"),
        }
    }
}

/// Information about an engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineInfo {
    /// Unique identifier
    pub id: EngineId,
    /// Human-readable name
    pub name: String,
    /// Version string
    pub version: String,
    /// Engine type
    pub engine_type: EngineType,
    /// Supported capabilities
    pub capabilities: CapabilitySet,
    /// Model name/path if applicable
    pub model: Option<String>,
    /// License of the engine/model
    pub license: Option<String>,
    /// Whether the model allows commercial use
    pub commercial_use: Option<bool>,
}

impl EngineInfo {
    /// Create a new engine info builder.
    pub fn builder(id: impl Into<EngineId>) -> EngineInfoBuilder {
        EngineInfoBuilder::new(id)
    }

    /// Check if this engine allows commercial use.
    pub fn allows_commercial_use(&self) -> bool {
        self.commercial_use.unwrap_or(false)
    }
}

/// Builder for EngineInfo.
pub struct EngineInfoBuilder {
    info: EngineInfo,
}

impl EngineInfoBuilder {
    /// Create a new builder with the given ID.
    pub fn new(id: impl Into<EngineId>) -> Self {
        Self {
            info: EngineInfo {
                id: id.into(),
                name: String::new(),
                version: String::from("0.0.0"),
                engine_type: EngineType::Native,
                capabilities: CapabilitySet::new(),
                model: None,
                license: None,
                commercial_use: None,
            },
        }
    }

    /// Set the engine name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.info.name = name.into();
        self
    }

    /// Set the version.
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.info.version = version.into();
        self
    }

    /// Set the engine type.
    pub fn engine_type(mut self, engine_type: EngineType) -> Self {
        self.info.engine_type = engine_type;
        self
    }

    /// Set the capabilities.
    pub fn capabilities(mut self, capabilities: CapabilitySet) -> Self {
        self.info.capabilities = capabilities;
        self
    }

    /// Set the model name/path.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.info.model = Some(model.into());
        self
    }

    /// Set the license.
    pub fn license(mut self, license: impl Into<String>) -> Self {
        self.info.license = Some(license.into());
        self
    }

    /// Set commercial use flag.
    pub fn commercial_use(mut self, allowed: bool) -> Self {
        self.info.commercial_use = Some(allowed);
        self
    }

    /// Build the EngineInfo.
    pub fn build(self) -> EngineInfo {
        self.info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;

    #[test]
    fn test_engine_id() {
        let id = EngineId::new("whisper-cpp");
        assert_eq!(id.as_str(), "whisper-cpp");
        assert_eq!(id.to_string(), "whisper-cpp");
    }

    #[test]
    fn test_engine_state() {
        assert!(EngineState::Ready.is_available());
        assert!(!EngineState::Processing.is_available());
        assert!(EngineState::Error.is_error());
        assert!(EngineState::Stopped.is_stopped());
    }

    #[test]
    fn test_engine_info_builder() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::StreamingStt).add(Capability::Gpu);

        let info = EngineInfo::builder("whisper-cpp")
            .name("Whisper.cpp")
            .version("1.5.0")
            .engine_type(EngineType::Native)
            .capabilities(caps)
            .model("ggml-base.bin")
            .license("MIT")
            .commercial_use(true)
            .build();

        assert_eq!(info.id.as_str(), "whisper-cpp");
        assert_eq!(info.name, "Whisper.cpp");
        assert!(info.allows_commercial_use());
    }
}
