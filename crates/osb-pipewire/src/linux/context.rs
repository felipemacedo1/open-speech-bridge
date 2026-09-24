//! PipeWire context and device enumeration.

use crate::device::{AudioDevice, AudioEnvironment, DeviceType};
use crate::error::{PipeWireError, Result};
use pipewire as pw;
use tracing::{debug, info};

/// PipeWire context for audio operations.
pub struct PipeWireContext {
    /// PipeWire main loop (kept alive)
    _mainloop: pw::main_loop::MainLoop,
    /// Library version
    version: String,
}

impl PipeWireContext {
    /// Create a new PipeWire context.
    pub fn new() -> Result<Self> {
        // Initialize PipeWire
        pw::init();

        // Get version from pipewire command
        let version = get_pipewire_version().unwrap_or_else(|| "unknown".to_string());
        info!(version = %version, "initialized PipeWire");

        // Create main loop
        let mainloop = pw::main_loop::MainLoop::new(None)
            .map_err(|e| PipeWireError::InitFailed(e.to_string()))?;

        Ok(Self {
            _mainloop: mainloop,
            version,
        })
    }

    /// Get the PipeWire library version.
    pub fn version(&self) -> Option<String> {
        Some(self.version.clone())
    }

    /// Enumerate audio devices of the specified type.
    pub fn enumerate_devices(&self, device_type: DeviceType) -> Result<Vec<AudioDevice>> {
        // For now, use pw-cli to list devices
        // In a full implementation, we'd use the registry listener
        let output = std::process::Command::new("pw-cli")
            .arg("list-objects")
            .output()
            .map_err(|e| PipeWireError::Internal(format!("failed to run pw-cli: {}", e)))?;

        if !output.status.success() {
            return Err(PipeWireError::ConnectionFailed(
                "pw-cli failed - is PipeWire running?".to_string(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let devices = parse_pw_cli_output(&stdout, device_type);

        debug!(count = devices.len(), ?device_type, "enumerated devices");
        Ok(devices)
    }

    /// Get complete audio environment information.
    pub fn get_environment(&self) -> Result<AudioEnvironment> {
        let input_devices = self.enumerate_devices(DeviceType::Input)?;
        let output_devices = self.enumerate_devices(DeviceType::Output)?;

        let default_input = input_devices
            .iter()
            .find(|d| d.is_default)
            .map(|d| d.id.clone());
        let default_output = output_devices
            .iter()
            .find(|d| d.is_default)
            .map(|d| d.id.clone());

        Ok(AudioEnvironment {
            pipewire_version: Some(self.version.clone()),
            pipewire_running: true,
            default_input,
            default_output,
            input_devices,
            output_devices,
        })
    }
}

/// Parse pw-cli list-objects output to extract devices.
fn parse_pw_cli_output(output: &str, device_type: DeviceType) -> Vec<AudioDevice> {
    let mut devices = Vec::new();
    let target_class = match device_type {
        DeviceType::Input | DeviceType::VirtualInput => "Audio/Source",
        DeviceType::Output | DeviceType::VirtualOutput => "Audio/Sink",
    };

    let mut current_id: Option<u32> = None;
    let mut current_name: Option<String> = None;
    let mut current_desc: Option<String> = None;
    let mut is_target_class = false;

    for line in output.lines() {
        let line = line.trim();

        // New object starts with "id X, type ..."
        if line.starts_with("id ") {
            // Save previous device if valid
            if let (Some(id), Some(name)) = (current_id.take(), current_name.take()) {
                if is_target_class {
                    let mut device = AudioDevice::new(id.to_string(), name, device_type);
                    if let Some(desc) = current_desc.take() {
                        device = device.with_description(desc);
                    }
                    device = device.with_node_id(id);
                    devices.push(device);
                }
            }

            // Parse new ID
            if let Some(id_str) = line.split(',').next() {
                if let Some(id) = id_str.strip_prefix("id ") {
                    current_id = id.trim().parse().ok();
                }
            }
            is_target_class = false;
            current_desc = None;
        }

        // Look for media.class property
        if line.contains("media.class") && line.contains(target_class) {
            is_target_class = true;
        }

        // Look for node.name
        if line.contains("node.name") {
            if let Some(name) = extract_property_value(line) {
                current_name = Some(name);
            }
        }

        // Look for node.description
        if line.contains("node.description") {
            if let Some(desc) = extract_property_value(line) {
                current_desc = Some(desc);
            }
        }
    }

    // Don't forget the last device
    if let (Some(id), Some(name)) = (current_id, current_name) {
        if is_target_class {
            let mut device = AudioDevice::new(id.to_string(), name, device_type);
            if let Some(desc) = current_desc {
                device = device.with_description(desc);
            }
            device = device.with_node_id(id);
            devices.push(device);
        }
    }

    devices
}

/// Extract value from a property line like 'node.name = "value"'
fn extract_property_value(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() == 2 {
        let value = parts[1].trim();
        // Remove quotes if present
        let value = value.trim_matches('"').trim_matches('\'');
        Some(value.to_string())
    } else {
        None
    }
}

/// Get PipeWire version from the pipewire command.
fn get_pipewire_version() -> Option<String> {
    std::process::Command::new("pipewire")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8(o.stdout).ok().and_then(|s| {
                // Parse "pipewire X.Y.Z" or similar
                s.lines()
                    .next()
                    .and_then(|line| line.split_whitespace().last())
                    .map(|v| v.to_string())
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_property_value() {
        assert_eq!(
            extract_property_value("node.name = \"My Device\""),
            Some("My Device".to_string())
        );
        assert_eq!(
            extract_property_value("node.name = 'test'"),
            Some("test".to_string())
        );
    }
}
