//! Runtime orchestration for the daemon.

use osb_core::config::Config;
use osb_core::error::Result;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

/// Runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    /// Not started
    Stopped,
    /// Starting up
    Starting,
    /// Running normally
    Running,
    /// Paused (audio streams stopped but engines loaded)
    Paused,
    /// Shutting down
    ShuttingDown,
    /// Error state
    Error,
}

/// Runtime configuration.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Core configuration
    pub config: Config,
    /// Enable virtual microphone
    pub enable_virtual_mic: bool,
    /// Enable virtual sink (for incoming audio)
    pub enable_virtual_sink: bool,
    /// Virtual microphone name
    pub virtual_mic_name: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            config: Config::default(),
            enable_virtual_mic: true,
            enable_virtual_sink: false,
            virtual_mic_name: "OpenSpeechBridge".to_string(),
        }
    }
}

/// The main runtime that orchestrates all components.
pub struct Runtime {
    config: RuntimeConfig,
    state: Arc<RwLock<RuntimeState>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl Runtime {
    /// Create a new runtime with the given configuration.
    pub fn new(config: RuntimeConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            config,
            state: Arc::new(RwLock::new(RuntimeState::Stopped)),
            shutdown_tx,
        }
    }

    /// Create a runtime with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(RuntimeConfig::default())
    }

    /// Get the current runtime state.
    pub async fn state(&self) -> RuntimeState {
        *self.state.read().await
    }

    /// Start the runtime.
    pub async fn start(&self) -> Result<()> {
        {
            let mut state = self.state.write().await;
            if *state != RuntimeState::Stopped {
                warn!("runtime already started");
                return Ok(());
            }
            *state = RuntimeState::Starting;
        }

        info!("starting OpenSpeechBridge runtime");

        // Initialize components
        // 1. Audio manager (capture, playback, virtual devices)
        // 2. Engine supervisor (load and manage ML engines)
        // 3. Processing pipeline (connect audio to engines)

        {
            let mut state = self.state.write().await;
            *state = RuntimeState::Running;
        }

        info!("runtime started successfully");
        Ok(())
    }

    /// Stop the runtime.
    pub async fn stop(&self) -> Result<()> {
        {
            let state = self.state.read().await;
            if *state == RuntimeState::Stopped {
                return Ok(());
            }
        }

        {
            let mut state = self.state.write().await;
            *state = RuntimeState::ShuttingDown;
        }

        info!("shutting down runtime");

        // Signal shutdown to all components
        let _ = self.shutdown_tx.send(());

        // Stop components in reverse order
        // 1. Processing pipeline
        // 2. Engine supervisor
        // 3. Audio manager

        {
            let mut state = self.state.write().await;
            *state = RuntimeState::Stopped;
        }

        info!("runtime stopped");
        Ok(())
    }

    /// Pause audio processing (keep engines loaded).
    pub async fn pause(&self) -> Result<()> {
        let mut state = self.state.write().await;
        if *state == RuntimeState::Running {
            *state = RuntimeState::Paused;
            info!("runtime paused");
        }
        Ok(())
    }

    /// Resume audio processing.
    pub async fn resume(&self) -> Result<()> {
        let mut state = self.state.write().await;
        if *state == RuntimeState::Paused {
            *state = RuntimeState::Running;
            info!("runtime resumed");
        }
        Ok(())
    }

    /// Get a shutdown receiver for graceful shutdown handling.
    pub fn shutdown_receiver(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Get the runtime configuration.
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_runtime_lifecycle() {
        let runtime = Runtime::with_defaults();

        assert_eq!(runtime.state().await, RuntimeState::Stopped);

        runtime.start().await.unwrap();
        assert_eq!(runtime.state().await, RuntimeState::Running);

        runtime.pause().await.unwrap();
        assert_eq!(runtime.state().await, RuntimeState::Paused);

        runtime.resume().await.unwrap();
        assert_eq!(runtime.state().await, RuntimeState::Running);

        runtime.stop().await.unwrap();
        assert_eq!(runtime.state().await, RuntimeState::Stopped);
    }
}
