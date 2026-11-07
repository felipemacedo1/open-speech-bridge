//! Runtime orchestration for the daemon.

use crate::engine_supervisor::{EngineSupervisor, RestartPolicy};
use crate::worker::WorkerSpec;
use osb_core::config::Config;
use osb_core::error::{ConfigError, Error, Result};
use osb_protocol::engine::{EngineInfo, EngineType};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, RwLock};
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
    supervisor: Arc<Mutex<EngineSupervisor>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl Runtime {
    /// Create a new runtime with the given configuration, registering any configured UDS workers.
    pub fn new(config: RuntimeConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let mut supervisor = EngineSupervisor::new();
        register_configured_workers(&mut supervisor, &config.config);

        Self {
            config,
            state: Arc::new(RwLock::new(RuntimeState::Stopped)),
            supervisor: Arc::new(Mutex::new(supervisor)),
            shutdown_tx,
        }
    }

    /// Load a TOML [`Config`] file from disk and construct a [`Runtime`].
    pub fn from_config_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(Error::Config(ConfigError::FileNotFound {
                path: path.display().to_string(),
            }));
        }
        let raw = std::fs::read_to_string(path)?;
        let parsed = Config::from_toml_str(&raw)?;
        Ok(Self::new(RuntimeConfig {
            config: parsed,
            ..RuntimeConfig::default()
        }))
    }

    /// Create a runtime with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(RuntimeConfig::default())
    }

    /// Access the runtime's [`EngineSupervisor`].
    pub fn supervisor(&self) -> &Arc<Mutex<EngineSupervisor>> {
        &self.supervisor
    }

    /// Get the current runtime state.
    pub async fn state(&self) -> RuntimeState {
        *self.state.read().await
    }

    /// Start the runtime and all registered engines.
    pub async fn start(&self) -> Result<()> {
        {
            let mut state = self.state.write().await;
            if *state != RuntimeState::Stopped && *state != RuntimeState::Error {
                warn!("runtime already started");
                return Ok(());
            }
            *state = RuntimeState::Starting;
        }

        info!("starting OpenSpeechBridge runtime");

        if let Err(err) = self.supervisor.lock().await.start_all().await {
            let mut state = self.state.write().await;
            *state = RuntimeState::Error;
            return Err(err);
        }

        {
            let mut state = self.state.write().await;
            *state = RuntimeState::Running;
        }

        info!("runtime started successfully");
        Ok(())
    }

    /// Stop the runtime and gracefully shut down all supervised engines.
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

        let _ = self.shutdown_tx.send(());
        let _ = self.supervisor.lock().await.stop_all().await;

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

fn register_configured_workers(supervisor: &mut EngineSupervisor, config: &Config) {
    let startup = Duration::from_millis(config.engine.startup_timeout_ms.max(1_000));
    let processing = Duration::from_millis(config.engine.processing_timeout_ms.max(1_000));

    for worker_cfg in &config.engine.workers {
        if worker_cfg.id.is_empty() || worker_cfg.program.is_empty() {
            continue;
        }
        let info = EngineInfo::builder(worker_cfg.id.clone())
            .name(if worker_cfg.name.is_empty() {
                worker_cfg.id.clone()
            } else {
                worker_cfg.name.clone()
            })
            .version("0.1.0")
            .engine_type(EngineType::External)
            .build();
        let spec = WorkerSpec::spawn(&worker_cfg.program, &worker_cfg.args)
            .with_startup_timeout(startup)
            .with_processing_timeout(processing);
        let policy = RestartPolicy {
            max_restarts: worker_cfg.max_restarts,
            ..RestartPolicy::default()
        };
        supervisor.register_worker(info, spec, policy);
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

    #[tokio::test]
    async fn test_runtime_from_config_file_registers_workers() {
        let tmp_path =
            std::env::temp_dir().join(format!("osb-runtime-cfg-test-{}.toml", std::process::id()));
        let toml = r#"
[engine]
source_language = "pt-BR"
target_language = "en-US"
speech_translation_engine = "whisper-small"
tts_engine = "kokoro-tts"

[[engine.workers]]
id = "whisper-small"
name = "Faster-Whisper Small"
program = "python3"
args = ["worker.py"]
max_restarts = 2
"#;
        std::fs::write(&tmp_path, toml).unwrap();

        let runtime = Runtime::from_config_file(&tmp_path).unwrap();
        let _ = std::fs::remove_file(&tmp_path);

        assert_eq!(runtime.config().config.engine.source_language, "pt-BR");
        let sup = runtime.supervisor().lock().await;
        assert_eq!(sup.list_engines().len(), 1);
    }
}
