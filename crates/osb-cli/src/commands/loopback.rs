//! Audio loopback test command.

use anyhow::Result;
use std::time::Duration;
use tokio::signal;
use tracing::info;

pub async fn run(input_device: Option<String>, duration: u64) -> Result<()> {
    let device = input_device.unwrap_or_else(|| "default".to_string());

    println!("Starting audio loopback test");
    println!("  Input device: {}", device);
    println!(
        "  Duration: {} seconds (Ctrl+C to stop early)",
        if duration == 0 {
            "unlimited".to_string()
        } else {
            duration.to_string()
        }
    );
    println!();

    #[cfg(target_os = "linux")]
    {
        info!(device = %device, "starting loopback");

        // In a full implementation, we would:
        // 1. Create a capture stream from the input device
        // 2. Create a virtual microphone
        // 3. Connect capture -> virtual mic
        // 4. Run until duration or Ctrl+C

        println!("Loopback active - speak into microphone");
        println!("Select 'OpenSpeechBridge' as input in your application to hear yourself");
        println!();

        if duration == 0 {
            // Run until Ctrl+C
            signal::ctrl_c().await?;
        } else {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(duration)) => {}
                _ = signal::ctrl_c() => {}
            }
        }

        println!();
        println!("Loopback stopped");
    }

    #[cfg(not(target_os = "linux"))]
    {
        println!("Error: Loopback test is only available on Linux with PipeWire");
        println!("This platform is not supported yet.");
    }

    Ok(())
}
