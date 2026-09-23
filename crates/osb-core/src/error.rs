//! Error types for OpenSpeechBridge.
//!
//! This module provides structured error handling across the runtime.
//! Errors are designed to be:
//! - Informative for debugging
//! - Recoverable where possible
//! - Suitable for both human display and programmatic handling

use thiserror::Error;

/// Result type alias using the crate's error type.
pub type Result<T> = std::result::Result<T, Error>;

/// Top-level error type for OpenSpeechBridge.
#[derive(Error, Debug)]
pub enum Error {
    /// Audio subsystem errors
    #[error("audio error: {0}")]
    Audio(#[from] AudioError),

    /// Engine-related errors
    #[error("engine error: {0}")]
    Engine(#[from] EngineError),

    /// Configuration errors
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Internal errors (bugs, unexpected states)
    #[error("internal error: {0}")]
    Internal(String),
}

/// Audio subsystem errors.
#[derive(Error, Debug)]
pub enum AudioError {
    /// Device not found
    #[error("device not found: {name}")]
    DeviceNotFound { name: String },

    /// Device access denied
    #[error("device access denied: {name} - {reason}")]
    DeviceAccessDenied { name: String, reason: String },

    /// Format not supported by device
    #[error("format not supported: {0}")]
    FormatNotSupported(String),

    /// Buffer overflow (data lost)
    #[error("buffer overflow: {dropped_frames} frames dropped")]
    BufferOverflow { dropped_frames: u64 },

    /// Buffer underrun (no data available)
    #[error("buffer underrun: {starved_frames} frames starved")]
    BufferUnderrun { starved_frames: u64 },

    /// Sample rate conversion failed
    #[error("resampling error: {0}")]
    ResamplingError(String),

    /// PipeWire-specific error
    #[error("PipeWire error: {0}")]
    PipeWire(String),

    /// Stream error
    #[error("stream error: {0}")]
    Stream(String),
}

/// Engine-related errors.
#[derive(Error, Debug)]
pub enum EngineError {
    /// Engine not found
    #[error("engine not found: {id}")]
    NotFound { id: String },

    /// Engine failed to start
    #[error("engine failed to start: {id} - {reason}")]
    StartFailed { id: String, reason: String },

    /// Engine crashed
    #[error("engine crashed: {id} - {reason}")]
    Crashed { id: String, reason: String },

    /// Engine timed out
    #[error("engine timeout: {id} after {timeout_ms}ms")]
    Timeout { id: String, timeout_ms: u64 },

    /// Engine protocol error
    #[error("protocol error: {0}")]
    Protocol(String),

    /// Capability not supported
    #[error("capability not supported: {capability} by engine {engine_id}")]
    CapabilityNotSupported { engine_id: String, capability: String },

    /// Model loading error
    #[error("model error: {0}")]
    Model(String),
}

/// Configuration errors.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// Invalid configuration value
    #[error("invalid configuration: {field} - {reason}")]
    Invalid { field: String, reason: String },

    /// Missing required field
    #[error("missing required configuration: {field}")]
    Missing { field: String },

    /// File not found
    #[error("configuration file not found: {path}")]
    FileNotFound { path: String },

    /// Parse error
    #[error("configuration parse error: {0}")]
    Parse(String),
}

/// Error severity for metrics and logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Transient error, automatic recovery expected
    Transient,
    /// Error requiring attention but not critical
    Warning,
    /// Critical error requiring immediate action
    Critical,
    /// Fatal error, system cannot continue
    Fatal,
}

impl Error {
    /// Get the severity of this error.
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Error::Audio(e) => e.severity(),
            Error::Engine(e) => e.severity(),
            Error::Config(_) => ErrorSeverity::Warning,
            Error::Io(_) => ErrorSeverity::Warning,
            Error::Internal(_) => ErrorSeverity::Critical,
        }
    }

    /// Whether this error is recoverable.
    pub fn is_recoverable(&self) -> bool {
        !matches!(self.severity(), ErrorSeverity::Fatal)
    }

    /// Create an internal error.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

impl AudioError {
    fn severity(&self) -> ErrorSeverity {
        match self {
            Self::BufferOverflow { .. } | Self::BufferUnderrun { .. } => ErrorSeverity::Transient,
            Self::DeviceNotFound { .. } | Self::FormatNotSupported(_) => ErrorSeverity::Warning,
            Self::DeviceAccessDenied { .. } => ErrorSeverity::Critical,
            Self::PipeWire(_) | Self::Stream(_) | Self::ResamplingError(_) => ErrorSeverity::Warning,
        }
    }
}

impl EngineError {
    fn severity(&self) -> ErrorSeverity {
        match self {
            Self::Timeout { .. } => ErrorSeverity::Transient,
            Self::NotFound { .. } | Self::CapabilityNotSupported { .. } => ErrorSeverity::Warning,
            Self::StartFailed { .. } | Self::Crashed { .. } => ErrorSeverity::Critical,
            Self::Protocol(_) | Self::Model(_) => ErrorSeverity::Warning,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_severity() {
        let overflow = Error::Audio(AudioError::BufferOverflow { dropped_frames: 10 });
        assert_eq!(overflow.severity(), ErrorSeverity::Transient);
        assert!(overflow.is_recoverable());

        let internal = Error::internal("unexpected state");
        assert_eq!(internal.severity(), ErrorSeverity::Critical);
    }

    #[test]
    fn test_error_display() {
        let err = AudioError::DeviceNotFound {
            name: "hw:0".to_string(),
        };
        assert!(err.to_string().contains("hw:0"));
    }
}
