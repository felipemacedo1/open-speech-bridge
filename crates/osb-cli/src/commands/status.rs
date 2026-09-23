//! Status command.

use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
struct StatusOutput {
    daemon_running: bool,
    runtime_state: String,
    audio: AudioStatus,
    engines: EngineStatus,
}

#[derive(Serialize)]
struct AudioStatus {
    capture_active: bool,
    playback_active: bool,
    virtual_mic_active: bool,
    buffer_health: String,
}

#[derive(Serialize)]
struct EngineStatus {
    loaded: usize,
    active: usize,
}

pub async fn run(json_output: bool) -> Result<()> {
    // In a full implementation, this would connect to the running daemon
    // via IPC and query its status

    let status = StatusOutput {
        daemon_running: false,
        runtime_state: "stopped".to_string(),
        audio: AudioStatus {
            capture_active: false,
            playback_active: false,
            virtual_mic_active: false,
            buffer_health: "n/a".to_string(),
        },
        engines: EngineStatus {
            loaded: 0,
            active: 0,
        },
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!("OpenSpeechBridge Status");
        println!("═══════════════════════");
        println!();

        let running_icon = if status.daemon_running { "●" } else { "○" };
        let running_text = if status.daemon_running {
            "running"
        } else {
            "stopped"
        };
        println!("Daemon: {} {}", running_icon, running_text);
        println!();

        println!("Audio:");
        println!(
            "  Capture:     {}",
            if status.audio.capture_active {
                "active"
            } else {
                "inactive"
            }
        );
        println!(
            "  Playback:    {}",
            if status.audio.playback_active {
                "active"
            } else {
                "inactive"
            }
        );
        println!(
            "  Virtual Mic: {}",
            if status.audio.virtual_mic_active {
                "active"
            } else {
                "inactive"
            }
        );
        println!();

        println!("Engines:");
        println!("  Loaded: {}", status.engines.loaded);
        println!("  Active: {}", status.engines.active);
        println!();

        if !status.daemon_running {
            println!("Daemon is not running. Start with: openspeechbridge run");
        }
    }

    Ok(())
}
