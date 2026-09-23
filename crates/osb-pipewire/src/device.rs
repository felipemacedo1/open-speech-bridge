//! Audio device representation.

use osb_core::audio::AudioFormat;
use serde::{Deserialize, Serialize};

/// Type of audio device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    /// Input device (microphone)
    Input,
    /// Output device (speakers/headphones)
    Output,
    /// Virtual input (for routing to applications)
    VirtualInput,
    /// Virtual output (for capturing from applications)
    VirtualOutput,
}

impl DeviceType {
    /// Check if this is an input device.
    pub fn is_input(&self) -> bool {
        matches!(self, Self::Input | Self::VirtualInput)
    }

    /// Check if this is an output device.
    pub fn is_output(&self) -> bool {
        matches!(self, Self::Output | Self::VirtualOutput)
    }

    /// Check if this is a virtual device.
    pub fn is_virtual(&self) -> bool {
        matches!(self, Self::VirtualInput | Self::VirtualOutput)
    }
}

/// Information about an audio device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    /// Unique device identifier
    pub id: String,
    /// Human-readable device name
    pub name: String,
    /// Device description
    pub description: Option<String>,
    /// Device type
    pub device_type: DeviceType,
    /// Whether this is the default device for its type
    pub is_default: bool,
    /// Supported audio format
    pub format: Option<AudioFormat>,
    /// PipeWire node ID (Linux-specific)
    pub node_id: Option<u32>,
}

impl AudioDevice {
    /// Create a new audio device.
    pub fn new(id: impl Into<String>, name: impl Into<String>, device_type: DeviceType) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            device_type,
            is_default: false,
            format: None,
            node_id: None,
        }
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set as default device.
    pub fn as_default(mut self) -> Self {
        self.is_default = true;
        self
    }

    /// Set the audio format.
    pub fn with_format(mut self, format: AudioFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// Set the PipeWire node ID.
    pub fn with_node_id(mut self, node_id: u32) -> Self {
        self.node_id = Some(node_id);
        self
    }
}

/// System audio environment information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioEnvironment {
    /// PipeWire version (if available)
    pub pipewire_version: Option<String>,
    /// Whether PipeWire is running
    pub pipewire_running: bool,
    /// Default input device
    pub default_input: Option<String>,
    /// Default output device
    pub default_output: Option<String>,
    /// Available input devices
    pub input_devices: Vec<AudioDevice>,
    /// Available output devices
    pub output_devices: Vec<AudioDevice>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_type() {
        assert!(DeviceType::Input.is_input());
        assert!(!DeviceType::Input.is_output());
        assert!(!DeviceType::Input.is_virtual());

        assert!(DeviceType::VirtualInput.is_input());
        assert!(DeviceType::VirtualInput.is_virtual());
    }

    #[test]
    fn test_audio_device_builder() {
        let device = AudioDevice::new("hw:0", "Built-in Microphone", DeviceType::Input)
            .with_description("Internal microphone")
            .as_default()
            .with_node_id(42);

        assert_eq!(device.id, "hw:0");
        assert!(device.is_default);
        assert_eq!(device.node_id, Some(42));
    }
}
