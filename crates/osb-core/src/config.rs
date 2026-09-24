//! Configuration types for OpenSpeechBridge.
//!
//! This module defines the runtime configuration structure.
//! Configuration can be loaded from TOML files or environment variables.

use crate::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use serde::{Deserialize, Serialize};

/// Main configuration structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Audio subsystem configuration
    pub audio: AudioConfig,
    /// Engine configuration
    pub engine: EngineConfig,
    /// Logging configuration
    pub logging: LoggingConfig,
    /// Metrics configuration
    pub metrics: MetricsConfig,
}

/// Audio subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Input device name (empty = default)
    pub input_device: String,
    /// Output device name (empty = default)
    pub output_device: String,
    /// Buffer size in milliseconds
    pub buffer_ms: u32,
    /// Target latency in milliseconds
    pub target_latency_ms: u32,
    /// Native audio format for capture/playback
    pub native_format: AudioFormat,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            input_device: String::new(),
            output_device: String::new(),
            buffer_ms: 20,
            target_latency_ms: 50,
            native_format: AudioFormat {
                sample_rate: SampleRate::PRO_48K,
                sample_format: SampleFormat::F32,
                channels: ChannelLayout::Stereo,
            },
        }
    }
}

/// Engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    /// Source language code (e.g., "pt-BR")
    pub source_language: String,
    /// Target language code (e.g., "en-US")
    pub target_language: String,
    /// STT engine identifier
    pub stt_engine: Option<String>,
    /// Translation engine identifier
    pub translation_engine: Option<String>,
    /// TTS engine identifier
    pub tts_engine: Option<String>,
    /// Speech-to-speech engine identifier (alternative to STT+MT+TTS)
    pub s2s_engine: Option<String>,
    /// Prefer GPU acceleration
    pub prefer_gpu: bool,
    /// Engine startup timeout in milliseconds
    pub startup_timeout_ms: u64,
    /// Engine processing timeout in milliseconds
    pub processing_timeout_ms: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            source_language: "pt-BR".to_string(),
            target_language: "en-US".to_string(),
            stt_engine: None,
            translation_engine: None,
            tts_engine: None,
            s2s_engine: None,
            prefer_gpu: true,
            startup_timeout_ms: 30_000,
            processing_timeout_ms: 10_000,
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,
    /// Enable JSON format
    pub json: bool,
    /// Log file path (empty = stderr only)
    pub file: Option<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            json: false,
            file: None,
        }
    }
}

/// Metrics configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,
    /// Prometheus exporter port (0 = disabled)
    pub prometheus_port: u16,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            prometheus_port: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.audio.buffer_ms, 20);
        assert_eq!(config.engine.source_language, "pt-BR");
        assert_eq!(config.logging.level, "info");
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml).unwrap();
        assert_eq!(parsed.audio.buffer_ms, config.audio.buffer_ms);
    }
}
