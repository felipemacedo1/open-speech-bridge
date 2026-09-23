//! System diagnostics command.

use anyhow::Result;
use serde::Serialize;
use std::process::Command;

#[derive(Serialize)]
struct DiagnosticReport {
    platform: PlatformInfo,
    pipewire: PipeWireInfo,
    rust: RustInfo,
    recommendations: Vec<String>,
}

#[derive(Serialize)]
struct PlatformInfo {
    os: String,
    arch: String,
    kernel: Option<String>,
    distro: Option<String>,
}

#[derive(Serialize)]
struct PipeWireInfo {
    available: bool,
    version: Option<String>,
    daemon_running: bool,
    wireplumber_running: bool,
}

#[derive(Serialize)]
struct RustInfo {
    version: String,
    target: String,
}

pub async fn run(json_output: bool) -> Result<()> {
    let report = collect_diagnostics();

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }

    Ok(())
}

fn collect_diagnostics() -> DiagnosticReport {
    let platform = collect_platform_info();
    let pipewire = collect_pipewire_info();
    let rust = collect_rust_info();
    let recommendations = generate_recommendations(&platform, &pipewire);

    DiagnosticReport {
        platform,
        pipewire,
        rust,
        recommendations,
    }
}

fn collect_platform_info() -> PlatformInfo {
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();

    let kernel = if cfg!(target_os = "linux") {
        Command::new("uname")
            .arg("-r")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
    } else {
        None
    };

    let distro = if cfg!(target_os = "linux") {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|l| l.starts_with("PRETTY_NAME="))
                    .map(|l| {
                        l.trim_start_matches("PRETTY_NAME=")
                            .trim_matches('"')
                            .to_string()
                    })
            })
    } else {
        None
    };

    PlatformInfo {
        os,
        arch,
        kernel,
        distro,
    }
}

fn collect_pipewire_info() -> PipeWireInfo {
    let mut info = PipeWireInfo {
        available: false,
        version: None,
        daemon_running: false,
        wireplumber_running: false,
    };

    #[cfg(target_os = "linux")]
    {
        // Check PipeWire version
        if let Ok(output) = Command::new("pipewire").arg("--version").output() {
            if output.status.success() {
                info.available = true;
                info.version = String::from_utf8(output.stdout)
                    .ok()
                    .map(|s| s.lines().next().unwrap_or("").trim().to_string());
            }
        }

        // Check if daemon is running
        if let Ok(output) = Command::new("pw-cli").arg("info").arg("0").output() {
            info.daemon_running = output.status.success();
        }

        // Check WirePlumber
        if let Ok(output) = Command::new("wpctl").arg("status").output() {
            info.wireplumber_running = output.status.success();
        }
    }

    info
}

fn collect_rust_info() -> RustInfo {
    RustInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        target: std::env::consts::ARCH.to_string(),
    }
}

fn generate_recommendations(platform: &PlatformInfo, pipewire: &PipeWireInfo) -> Vec<String> {
    let mut recs = Vec::new();

    if platform.os != "linux" {
        recs.push(
            "OpenSpeechBridge currently only supports Linux. Windows/macOS support is planned."
                .to_string(),
        );
    }

    #[cfg(target_os = "linux")]
    {
        if !pipewire.available {
            recs.push("PipeWire is not installed. Install it with: sudo apt install pipewire pipewire-audio-client-libraries".to_string());
        } else if !pipewire.daemon_running {
            recs.push(
                "PipeWire daemon is not running. Start it with: systemctl --user start pipewire"
                    .to_string(),
            );
        }

        if !pipewire.wireplumber_running {
            recs.push(
                "WirePlumber is not running. Install/start it for better device management."
                    .to_string(),
            );
        }
    }

    if recs.is_empty() {
        recs.push("System looks ready for OpenSpeechBridge!".to_string());
    }

    recs
}

fn print_report(report: &DiagnosticReport) {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║           OpenSpeechBridge System Diagnostics            ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    println!("Platform:");
    println!("  OS:           {}", report.platform.os);
    println!("  Architecture: {}", report.platform.arch);
    if let Some(ref kernel) = report.platform.kernel {
        println!("  Kernel:       {}", kernel);
    }
    if let Some(ref distro) = report.platform.distro {
        println!("  Distribution: {}", distro);
    }
    println!();

    println!("PipeWire:");
    let status = |b| if b { "✓" } else { "✗" };
    println!("  Installed:    {}", status(report.pipewire.available));
    if let Some(ref version) = report.pipewire.version {
        println!("  Version:      {}", version);
    }
    println!("  Daemon:       {}", status(report.pipewire.daemon_running));
    println!(
        "  WirePlumber:  {}",
        status(report.pipewire.wireplumber_running)
    );
    println!();

    println!("OpenSpeechBridge:");
    println!("  Version:      {}", report.rust.version);
    println!("  Target:       {}", report.rust.target);
    println!();

    println!("Recommendations:");
    for rec in &report.recommendations {
        println!("  • {}", rec);
    }
}
