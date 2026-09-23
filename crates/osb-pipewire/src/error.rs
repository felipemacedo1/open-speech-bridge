//! PipeWire-specific error types.

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

impl From<PipeWireError> for osb_core::error::AudioError {
    fn from(err: PipeWireError) -> Self {
        osb_core::error::AudioError::PipeWire(err.to_string())
    }
}
