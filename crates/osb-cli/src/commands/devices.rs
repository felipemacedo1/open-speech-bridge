//! List audio devices command.

use anyhow::Result;
use osb_daemon::audio_manager::AudioManager;
use serde::Serialize;

#[derive(Serialize)]
struct DeviceList {
    input_devices: Vec<DeviceInfo>,
    output_devices: Vec<DeviceInfo>,
}

#[derive(Serialize)]
struct DeviceInfo {
    id: String,
    name: String,
    description: Option<String>,
    is_default: bool,
}

pub async fn run(input_only: bool, output_only: bool, json_output: bool) -> Result<()> {
    let mut manager = AudioManager::new();
    manager.initialize()?;

    let show_input = !output_only;
    let show_output = !input_only;

    if json_output {
        let list = DeviceList {
            input_devices: if show_input {
                manager
                    .input_devices()
                    .iter()
                    .map(|d| DeviceInfo {
                        id: d.id.clone(),
                        name: d.name.clone(),
                        description: d.description.clone(),
                        is_default: d.is_default,
                    })
                    .collect()
            } else {
                vec![]
            },
            output_devices: if show_output {
                manager
                    .output_devices()
                    .iter()
                    .map(|d| DeviceInfo {
                        id: d.id.clone(),
                        name: d.name.clone(),
                        description: d.description.clone(),
                        is_default: d.is_default,
                    })
                    .collect()
            } else {
                vec![]
            },
        };
        println!("{}", serde_json::to_string_pretty(&list)?);
    } else {
        if show_input {
            println!("Input Devices:");
            println!("──────────────");
            let inputs = manager.input_devices();
            if inputs.is_empty() {
                println!("  No input devices found");
            } else {
                for device in inputs {
                    let default_marker = if device.is_default { " (default)" } else { "" };
                    println!("  [{}] {}{}", device.id, device.name, default_marker);
                    if let Some(ref desc) = device.description {
                        println!("      {}", desc);
                    }
                }
            }
            println!();
        }

        if show_output {
            println!("Output Devices:");
            println!("───────────────");
            let outputs = manager.output_devices();
            if outputs.is_empty() {
                println!("  No output devices found");
            } else {
                for device in outputs {
                    let default_marker = if device.is_default { " (default)" } else { "" };
                    println!("  [{}] {}{}", device.id, device.name, default_marker);
                    if let Some(ref desc) = device.description {
                        println!("      {}", desc);
                    }
                }
            }
        }
    }

    Ok(())
}
