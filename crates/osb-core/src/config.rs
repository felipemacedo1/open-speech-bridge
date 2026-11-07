//! Configuration types for OpenSpeechBridge.
//!
//! This module defines the runtime configuration structure.
//! Configuration can be parsed from TOML strings without performing I/O.

use crate::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use crate::error::{ConfigError, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

impl Config {
    /// Parse a [`Config`] from a TOML string.
    pub fn from_toml_str(input: &str) -> Result<Self> {
        toml::from_str(input).map_err(|e| Error::Config(ConfigError::parse(e.to_string())))
    }

    /// Serialize this [`Config`] to a pretty-printed TOML string.
    pub fn to_toml_string(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|e| Error::Config(ConfigError::parse(e.to_string())))
    }
}

/// Voice Activity Detection (VAD) tuning parameters in TOML configuration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VadConfigParams {
    /// Analysis frame duration in milliseconds
    pub frame_ms: u32,
    /// RMS threshold required to trigger speech onset from `Idle`
    pub start_rms_threshold: f32,
    /// Lower hysteresis RMS threshold sustaining active speech
    pub continue_rms_threshold: f32,
    /// Number of pre-speech frames retained in a bounded circular buffer
    pub pre_roll_frames: usize,
    /// Minimum voiced duration in milliseconds required before emitting an utterance
    pub min_speech_ms: u32,
    /// Trailing silence duration in milliseconds that closes an utterance
    pub end_silence_ms: u32,
    /// Hard upper bound on utterance duration in milliseconds
    pub max_utterance_ms: u32,
}

impl Default for VadConfigParams {
    fn default() -> Self {
        Self {
            frame_ms: 20,
            start_rms_threshold: 0.015,
            continue_rms_threshold: 0.008,
            pre_roll_frames: 5,
            min_speech_ms: 160,
            end_silence_ms: 360,
            max_utterance_ms: 15_000,
        }
    }
}

/// Audio subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Input device name (empty = default)
    pub input_device: String,
    /// Output device name (empty = default)
    pub output_device: String,
    /// Virtual microphone node description published in PipeWire
    pub virtual_mic_name: String,
    /// Virtual sink node description published in PipeWire for incoming call audio
    pub virtual_sink_name: String,
    /// Enable outgoing virtual microphone (`mic -> translated virtual mic`)
    pub enable_virtual_mic: bool,
    /// Enable incoming virtual sink (`app audio -> translated local playback`)
    pub enable_virtual_sink: bool,
    /// Buffer size in milliseconds
    pub buffer_ms: u32,
    /// Target latency in milliseconds
    pub target_latency_ms: u32,
    /// Native audio format for capture/playback
    pub native_format: AudioFormat,
    /// Voice Activity Detection segmentation parameters
    pub vad: VadConfigParams,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            input_device: String::new(),
            output_device: String::new(),
            virtual_mic_name: "OpenSpeechBridge".to_string(),
            virtual_sink_name: "OpenSpeechBridge Incoming".to_string(),
            enable_virtual_mic: true,
            enable_virtual_sink: false,
            buffer_ms: 20,
            target_latency_ms: 50,
            native_format: AudioFormat {
                sample_rate: SampleRate::PRO_48K,
                sample_format: SampleFormat::F32,
                channels: ChannelLayout::Stereo,
            },
            vad: VadConfigParams::default(),
        }
    }
}

/// External persistent UDS worker command specification in configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkerProcessConfig {
    /// Unique engine ID to register
    pub id: String,
    /// Human-readable engine name
    pub name: String,
    /// Program executable (e.g., python3 or worker binary)
    pub program: String,
    /// Command-line arguments passed before `--socket <path>`
    pub args: Vec<String>,
    /// Extra environment variables injected into the worker process
    pub env: BTreeMap<String, String>,
    /// Maximum automatic crash restarts
    pub max_restarts: u32,
}

/// Engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    /// Source language code (e.g., "pt-BR")
    pub source_language: String,
    /// Target language code (e.g., "en-US")
    pub target_language: String,
    /// Direct speech-translation engine identifier (e.g., Whisper `translate`)
    pub speech_translation_engine: Option<String>,
    /// STT engine identifier
    pub stt_engine: Option<String>,
    /// Translation engine identifier
    pub translation_engine: Option<String>,
    /// TTS engine identifier
    pub tts_engine: Option<String>,
    /// Speech-to-speech engine identifier (alternative to STT+MT+TTS)
    pub s2s_engine: Option<String>,
    /// Preferred TTS voice identifier for outgoing translation (`target_language`)
    pub voice_id: Option<String>,
    /// Preferred TTS voice identifier for incoming full-duplex translation (`source_language`)
    pub incoming_voice_id: Option<String>,
    /// Enable clause-chunked incremental TTS synthesis (M5 time-to-first-audio optimization)
    pub chunked_tts: bool,
    /// Minimum words per clause before splitting on comma/semicolon boundaries
    pub min_clause_words: usize,
    /// Enable simultaneous incoming call translation (`target_language -> source_language`, M4 Full Duplex)
    pub enable_incoming: bool,
    /// Prefer GPU acceleration
    pub prefer_gpu: bool,
    /// Engine startup timeout in milliseconds
    pub startup_timeout_ms: u64,
    /// Engine processing timeout in milliseconds
    pub processing_timeout_ms: u64,
    /// Configured external persistent UDS workers to supervise
    pub workers: Vec<WorkerProcessConfig>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            source_language: "pt-BR".to_string(),
            target_language: "en-US".to_string(),
            speech_translation_engine: None,
            stt_engine: None,
            translation_engine: None,
            tts_engine: None,
            s2s_engine: None,
            voice_id: None,
            incoming_voice_id: None,
            chunked_tts: true,
            min_clause_words: 3,
            enable_incoming: false,
            prefer_gpu: true,
            startup_timeout_ms: 30_000,
            processing_timeout_ms: 15_000,
            workers: Vec::new(),
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
        assert!(config.engine.chunked_tts);
        assert_eq!(config.logging.level, "info");
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml = config.to_toml_string().unwrap();
        let parsed = Config::from_toml_str(&toml).unwrap();
        assert_eq!(parsed.audio.buffer_ms, config.audio.buffer_ms);
        assert_eq!(
            parsed.audio.vad.end_silence_ms,
            config.audio.vad.end_silence_ms
        );
    }

    #[test]
    fn test_config_parse_invalid_toml_returns_config_error() {
        let err = Config::from_toml_str("[audio\nbuffer_ms = ???").unwrap_err();
        assert!(matches!(err, Error::Config(ConfigError::Parse(_))));
    }
}
