//! Audio device and stream management.

use osb_core::error::Result;
use osb_pipewire::device::{AudioDevice, AudioEnvironment};
use tracing::info;

/// Manages audio devices and streams.
pub struct AudioManager {
    environment: Option<AudioEnvironment>,
}

impl AudioManager {
    /// Create a new audio manager.
    pub fn new() -> Self {
        Self { environment: None }
    }

    /// Initialize the audio manager and discover devices.
    pub fn initialize(&mut self) -> Result<()> {
        info!("initializing audio manager");

        #[cfg(target_os = "linux")]
        {
            use osb_pipewire::PipeWireContext;

            match PipeWireContext::new() {
                Ok(ctx) => match ctx.get_environment() {
                    Ok(env) => {
                        info!(
                            pipewire_version = ?env.pipewire_version,
                            inputs = env.input_devices.len(),
                            outputs = env.output_devices.len(),
                            "audio environment discovered"
                        );
                        self.environment = Some(env);
                    }
                    Err(e) => {
                        tracing::warn!("failed to get audio environment: {}", e);
                    }
                },
                Err(e) => {
                    tracing::warn!("failed to initialize PipeWire: {}", e);
                }
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            tracing::warn!("audio not available on this platform");
        }

        Ok(())
    }

    /// Get the current audio environment.
    pub fn environment(&self) -> Option<&AudioEnvironment> {
        self.environment.as_ref()
    }

    /// Get input devices.
    pub fn input_devices(&self) -> Vec<&AudioDevice> {
        self.environment
            .as_ref()
            .map(|e| e.input_devices.iter().collect())
            .unwrap_or_default()
    }

    /// Get output devices.
    pub fn output_devices(&self) -> Vec<&AudioDevice> {
        self.environment
            .as_ref()
            .map(|e| e.output_devices.iter().collect())
            .unwrap_or_default()
    }

    /// Refresh device list.
    pub fn refresh_devices(&mut self) -> Result<()> {
        self.initialize()
    }
}

impl Default for AudioManager {
    fn default() -> Self {
        Self::new()
    }
}
