//! PipeWire-specific error types.
//!
//! This module provides structured error handling for PipeWire operations.
//! Errors are designed to be informative and to integrate well with the
//! core error types.

use thiserror::Error;

/// Result type for PipeWire operations.
pub type Result<T> = std::result::Result<T, PipeWireError>;

/// PipeWire-specific errors.
#[derive(Error, Debug)]
pub enum PipeWireError {
    /// PipeWire is not available on this platform
    #[error("PipeWire is not available on this platform")]
    NotAvailable,

    /// Failed to initialize PipeWire
    #[error("failed to initialize PipeWire: {0}")]
    InitFailed(String),

    /// Failed to connect to PipeWire daemon
    #[error("failed to connect to PipeWire daemon: {0}")]
    ConnectionFailed(String),

    /// Device not found
    #[error("device not found: {0}")]
    DeviceNotFound(String),

    /// Failed to create stream
    #[error("failed to create stream: {0}")]
    StreamCreationFailed(String),

    /// Stream error during operation
    #[error("stream error: {0}")]
    StreamError(String),

    /// Failed to create virtual device
    #[error("failed to create virtual device: {0}")]
    VirtualDeviceFailed(String),

    /// Permission denied
    #[error("permission denied: {0}")]
    PermissionDenied(String),

    /// Invalid configuration
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    /// Operation timed out
    #[error("operation timed out")]
    Timeout,

    /// Internal error
    #[error("internal error: {0}")]
    Internal(String),
}

impl PipeWireError {
    /// Create a "not available" error for non-Linux platforms.
    pub fn not_available() -> Self {
        Self::NotAvailable
    }

    /// Create an initialization error with context.
    pub fn init_failed(reason: impl Into<String>) -> Self {
        Self::InitFailed(reason.into())
    }

    /// Create a connection error with context.
    pub fn connection_failed(reason: impl Into<String>) -> Self {
        Self::ConnectionFailed(reason.into())
    }

    /// Create a device not found error.
    pub fn device_not_found(device: impl Into<String>) -> Self {
        Self::DeviceNotFound(device.into())
    }

    /// Create a stream creation error.
    pub fn stream_creation_failed(reason: impl Into<String>) -> Self {
        Self::StreamCreationFailed(reason.into())
    }

    /// Create a stream error.
    pub fn stream_error(reason: impl Into<String>) -> Self {
        Self::StreamError(reason.into())
    }

    /// Create a virtual device error.
    pub fn virtual_device_failed(reason: impl Into<String>) -> Self {
        Self::VirtualDeviceFailed(reason.into())
    }

    /// Create a permission denied error.
    pub fn permission_denied(reason: impl Into<String>) -> Self {
        Self::PermissionDenied(reason.into())
    }

    /// Create an invalid configuration error.
    pub fn invalid_config(reason: impl Into<String>) -> Self {
        Self::InvalidConfig(reason.into())
    }

    /// Create an internal error.
    pub fn internal(reason: impl Into<String>) -> Self {
        Self::Internal(reason.into())
    }

    /// Check if this error is recoverable.
    ///
    /// Recoverable errors are transient and the operation may succeed if retried.
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Self::Timeout | Self::ConnectionFailed(_))
    }

    /// Check if this error indicates a configuration problem.
    ///
    /// Configuration errors require user intervention to fix.
    pub fn is_config_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidConfig(_) | Self::DeviceNotFound(_) | Self::NotAvailable
        )
    }

    /// Check if this error indicates a permission problem.
    pub fn is_permission_error(&self) -> bool {
        matches!(self, Self::PermissionDenied(_))
    }
}

impl From<PipeWireError> for osb_core::error::AudioError {
    fn from(err: PipeWireError) -> Self {
        osb_core::error::AudioError::PipeWire(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = PipeWireError::device_not_found("hw:0");
        assert!(err.to_string().contains("hw:0"));
        assert!(err.to_string().contains("device not found"));
    }

    #[test]
    fn test_error_constructors() {
        let _ = PipeWireError::not_available();
        let _ = PipeWireError::init_failed("test");
        let _ = PipeWireError::connection_failed("test");
        let _ = PipeWireError::device_not_found("test");
        let _ = PipeWireError::stream_creation_failed("test");
        let _ = PipeWireError::stream_error("test");
        let _ = PipeWireError::virtual_device_failed("test");
        let _ = PipeWireError::permission_denied("test");
        let _ = PipeWireError::invalid_config("test");
        let _ = PipeWireError::internal("test");
    }

    #[test]
    fn test_is_recoverable() {
        assert!(PipeWireError::Timeout.is_recoverable());
        assert!(PipeWireError::connection_failed("test").is_recoverable());
        assert!(!PipeWireError::not_available().is_recoverable());
        assert!(!PipeWireError::permission_denied("test").is_recoverable());
    }

    #[test]
    fn test_is_config_error() {
        assert!(PipeWireError::not_available().is_config_error());
        assert!(PipeWireError::invalid_config("test").is_config_error());
        assert!(PipeWireError::device_not_found("test").is_config_error());
        assert!(!PipeWireError::Timeout.is_config_error());
    }

    #[test]
    fn test_is_permission_error() {
        assert!(PipeWireError::permission_denied("test").is_permission_error());
        assert!(!PipeWireError::Timeout.is_permission_error());
    }

    #[test]
    fn test_conversion_to_audio_error() {
        let pw_err = PipeWireError::device_not_found("test-device");
        let audio_err: osb_core::error::AudioError = pw_err.into();
        assert!(audio_err.to_string().contains("test-device"));
    }
}
