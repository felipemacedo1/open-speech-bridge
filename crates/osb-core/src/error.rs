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
    CapabilityNotSupported {
        engine_id: String,
        capability: String,
    },

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
    /// Get the severity of this audio error.
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::BufferOverflow { .. } | Self::BufferUnderrun { .. } => ErrorSeverity::Transient,
            Self::DeviceNotFound { .. } | Self::FormatNotSupported(_) => ErrorSeverity::Warning,
            Self::DeviceAccessDenied { .. } => ErrorSeverity::Critical,
            Self::PipeWire(_) | Self::Stream(_) | Self::ResamplingError(_) => {
                ErrorSeverity::Warning
            }
        }
    }

    /// Create a device not found error.
    pub fn device_not_found(name: impl Into<String>) -> Self {
        Self::DeviceNotFound { name: name.into() }
    }

    /// Create a device access denied error.
    pub fn device_access_denied(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::DeviceAccessDenied {
            name: name.into(),
            reason: reason.into(),
        }
    }

    /// Create a format not supported error.
    pub fn format_not_supported(desc: impl Into<String>) -> Self {
        Self::FormatNotSupported(desc.into())
    }

    /// Create a buffer overflow error.
    pub fn buffer_overflow(dropped_frames: u64) -> Self {
        Self::BufferOverflow { dropped_frames }
    }

    /// Create a buffer underrun error.
    pub fn buffer_underrun(starved_frames: u64) -> Self {
        Self::BufferUnderrun { starved_frames }
    }

    /// Create a resampling error.
    pub fn resampling_error(reason: impl Into<String>) -> Self {
        Self::ResamplingError(reason.into())
    }

    /// Create a PipeWire error.
    pub fn pipewire(reason: impl Into<String>) -> Self {
        Self::PipeWire(reason.into())
    }

    /// Create a stream error.
    pub fn stream(reason: impl Into<String>) -> Self {
        Self::Stream(reason.into())
    }

    /// Check if this is a transient error that may resolve itself.
    pub fn is_transient(&self) -> bool {
        matches!(self.severity(), ErrorSeverity::Transient)
    }
}

impl EngineError {
    /// Get the severity of this engine error.
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::Timeout { .. } => ErrorSeverity::Transient,
            Self::NotFound { .. } | Self::CapabilityNotSupported { .. } => ErrorSeverity::Warning,
            Self::StartFailed { .. } | Self::Crashed { .. } => ErrorSeverity::Critical,
            Self::Protocol(_) | Self::Model(_) => ErrorSeverity::Warning,
        }
    }

    /// Create an engine not found error.
    pub fn not_found(id: impl Into<String>) -> Self {
        Self::NotFound { id: id.into() }
    }

    /// Create an engine start failed error.
    pub fn start_failed(id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::StartFailed {
            id: id.into(),
            reason: reason.into(),
        }
    }

    /// Create an engine crashed error.
    pub fn crashed(id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Crashed {
            id: id.into(),
            reason: reason.into(),
        }
    }

    /// Create an engine timeout error.
    pub fn timeout(id: impl Into<String>, timeout_ms: u64) -> Self {
        Self::Timeout {
            id: id.into(),
            timeout_ms,
        }
    }

    /// Create a protocol error.
    pub fn protocol(reason: impl Into<String>) -> Self {
        Self::Protocol(reason.into())
    }

    /// Create a capability not supported error.
    pub fn capability_not_supported(
        engine_id: impl Into<String>,
        capability: impl Into<String>,
    ) -> Self {
        Self::CapabilityNotSupported {
            engine_id: engine_id.into(),
            capability: capability.into(),
        }
    }

    /// Create a model error.
    pub fn model(reason: impl Into<String>) -> Self {
        Self::Model(reason.into())
    }

    /// Check if this error indicates the engine should be restarted.
    pub fn should_restart(&self) -> bool {
        matches!(self, Self::Crashed { .. } | Self::Timeout { .. })
    }
}

impl ConfigError {
    /// Create an invalid configuration error.
    pub fn invalid(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// Create a missing field error.
    pub fn missing(field: impl Into<String>) -> Self {
        Self::Missing {
            field: field.into(),
        }
    }

    /// Create a file not found error.
    pub fn file_not_found(path: impl Into<String>) -> Self {
        Self::FileNotFound { path: path.into() }
    }

    /// Create a parse error.
    pub fn parse(reason: impl Into<String>) -> Self {
        Self::Parse(reason.into())
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

    #[test]
    fn test_audio_error_constructors() {
        let _ = AudioError::device_not_found("test");
        let _ = AudioError::device_access_denied("test", "reason");
        let _ = AudioError::format_not_supported("test");
        let _ = AudioError::buffer_overflow(100);
        let _ = AudioError::buffer_underrun(50);
        let _ = AudioError::resampling_error("test");
        let _ = AudioError::pipewire("test");
        let _ = AudioError::stream("test");
    }

    #[test]
    fn test_audio_error_transient() {
        assert!(AudioError::buffer_overflow(10).is_transient());
        assert!(AudioError::buffer_underrun(10).is_transient());
        assert!(!AudioError::device_not_found("test").is_transient());
    }

    #[test]
    fn test_engine_error_constructors() {
        let _ = EngineError::not_found("test");
        let _ = EngineError::start_failed("test", "reason");
        let _ = EngineError::crashed("test", "reason");
        let _ = EngineError::timeout("test", 1000);
        let _ = EngineError::protocol("test");
        let _ = EngineError::capability_not_supported("engine", "cap");
        let _ = EngineError::model("test");
    }

    #[test]
    fn test_engine_error_should_restart() {
        assert!(EngineError::crashed("test", "reason").should_restart());
        assert!(EngineError::timeout("test", 1000).should_restart());
        assert!(!EngineError::not_found("test").should_restart());
    }

    #[test]
    fn test_config_error_constructors() {
        let _ = ConfigError::invalid("field", "reason");
        let _ = ConfigError::missing("field");
        let _ = ConfigError::file_not_found("/path");
        let _ = ConfigError::parse("reason");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn test_error_from_audio() {
        let audio_err = AudioError::device_not_found("test");
        let err: Error = audio_err.into();
        assert!(matches!(err, Error::Audio(_)));
    }

    #[test]
    fn test_error_from_engine() {
        let engine_err = EngineError::not_found("test");
        let err: Error = engine_err.into();
        assert!(matches!(err, Error::Engine(_)));
    }

    #[test]
    fn test_error_from_config() {
        let config_err = ConfigError::missing("field");
        let err: Error = config_err.into();
        assert!(matches!(err, Error::Config(_)));
    }
}
