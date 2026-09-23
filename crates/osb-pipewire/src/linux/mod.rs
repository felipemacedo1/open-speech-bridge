//! Linux PipeWire implementation.
//!
//! This module provides the actual PipeWire integration for Linux systems.

mod context;
mod capture;
mod playback;
mod virtual_device;

pub use context::PipeWireContext;
pub use capture::{CaptureStream, CaptureStreamBuilder};
pub use playback::{PlaybackStream, PlaybackStreamBuilder};
pub use virtual_device::{
    VirtualMicrophone, VirtualSink,
    DEFAULT_VIRTUAL_MIC_NAME, DEFAULT_VIRTUAL_SINK_NAME,
};

use std::process::Command;

/// Check if PipeWire is available on this system.
pub fn is_available() -> bool {
    // Check if pipewire daemon is running
    Command::new("pw-cli")
        .arg("info")
        .arg("0")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get diagnostic information about PipeWire.
pub fn diagnose() -> String {
    let mut info = String::new();
    
    // Check PipeWire version
    if let Ok(output) = Command::new("pipewire").arg("--version").output() {
        if output.status.success() {
            info.push_str("PipeWire: ");
            info.push_str(String::from_utf8_lossy(&output.stdout).trim());
            info.push('\n');
        }
    } else {
        info.push_str("PipeWire: not found in PATH\n");
    }

    // Check if daemon is running
    if let Ok(output) = Command::new("pw-cli").arg("info").arg("0").output() {
        if output.status.success() {
            info.push_str("Daemon: running\n");
        } else {
            info.push_str("Daemon: not running\n");
        }
    } else {
        info.push_str("Daemon: cannot check (pw-cli not available)\n");
    }

    // Check WirePlumber
    if let Ok(output) = Command::new("wpctl").arg("status").output() {
        if output.status.success() {
            info.push_str("WirePlumber: running\n");
        } else {
            info.push_str("WirePlumber: not running\n");
        }
    } else {
        info.push_str("WirePlumber: not available\n");
    }

    info
}
