//! Stub implementation for non-Linux platforms.
//!
//! This module provides placeholder implementations that return
//! appropriate errors when PipeWire is not available.

use crate::device::{AudioDevice, AudioEnvironment, DeviceType};
use crate::error::{PipeWireError, Result};

/// PipeWire context (stub).
pub struct PipeWireContext;

impl PipeWireContext {
    /// Create a new PipeWire context.
    ///
    /// On non-Linux platforms, this returns an error.
    pub fn new() -> Result<Self> {
        Err(PipeWireError::NotAvailable)
    }

    /// Enumerate audio devices.
    pub fn enumerate_devices(&self, _device_type: DeviceType) -> Result<Vec<AudioDevice>> {
        Err(PipeWireError::NotAvailable)
    }

    /// Get the audio environment.
    pub fn get_environment(&self) -> Result<AudioEnvironment> {
        Err(PipeWireError::NotAvailable)
    }

    /// Get PipeWire version.
    pub fn version(&self) -> Option<String> {
        None
    }
}

/// Audio capture stream (stub).
pub struct CaptureStream;

impl CaptureStream {
    /// Create a new capture stream.
    pub fn new(_ctx: &PipeWireContext, _device: &str) -> Result<Self> {
        Err(PipeWireError::NotAvailable)
    }

    /// Start capturing.
    pub fn start(&mut self) -> Result<()> {
        Err(PipeWireError::NotAvailable)
    }

    /// Stop capturing.
    pub fn stop(&mut self) -> Result<()> {
        Err(PipeWireError::NotAvailable)
    }
}

/// Audio playback stream (stub).
pub struct PlaybackStream;

impl PlaybackStream {
    /// Create a new playback stream.
    pub fn new(_ctx: &PipeWireContext, _device: &str) -> Result<Self> {
        Err(PipeWireError::NotAvailable)
    }

    /// Start playback.
    pub fn start(&mut self) -> Result<()> {
        Err(PipeWireError::NotAvailable)
    }

    /// Stop playback.
    pub fn stop(&mut self) -> Result<()> {
        Err(PipeWireError::NotAvailable)
    }
}

/// Virtual microphone (stub).
pub struct VirtualMicrophone;

impl VirtualMicrophone {
    /// Create a new virtual microphone.
    pub fn new(_ctx: &PipeWireContext, _name: &str) -> Result<Self> {
        Err(PipeWireError::NotAvailable)
    }

    /// Start the virtual microphone.
    pub fn start(&mut self) -> Result<()> {
        Err(PipeWireError::NotAvailable)
    }

    /// Stop the virtual microphone.
    pub fn stop(&mut self) -> Result<()> {
        Err(PipeWireError::NotAvailable)
    }
}

/// Check if PipeWire is available on this platform.
pub fn is_available() -> bool {
    false
}

/// Get diagnostic information about PipeWire availability.
pub fn diagnose() -> String {
    "PipeWire is only available on Linux. This platform is not supported.".to_string()
}
