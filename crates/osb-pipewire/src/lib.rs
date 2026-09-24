//! # osb-pipewire
//!
//! PipeWire audio backend for OpenSpeechBridge.
//!
//! This crate provides PipeWire integration for audio capture, playback,
//! and virtual device creation on Linux systems.
//!
//! ## Platform Support
//!
//! This crate only compiles on Linux. On other platforms, it provides
//! stub implementations that return appropriate errors.
//!
//! ## Features
//!
//! - Device enumeration
//! - Audio capture from physical microphones
//! - Audio playback to physical outputs
//! - Virtual microphone creation for application integration
//! - Virtual sink for capturing application audio
//!
//! ## Example
//!
//! ```ignore
//! use osb_pipewire::{PipeWireContext, DeviceType};
//!
//! let ctx = PipeWireContext::new()?;
//! let devices = ctx.enumerate_devices(DeviceType::Input)?;
//! for device in devices {
//!     println!("{}: {}", device.id, device.name);
//! }
//! ```

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(not(target_os = "linux"))]
mod stub;

#[cfg(not(target_os = "linux"))]
pub use stub::*;

pub mod device;
pub mod error;

pub use device::{AudioDevice, DeviceType};
pub use error::{PipeWireError, Result};
