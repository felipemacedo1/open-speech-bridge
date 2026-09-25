//! Protocol messages between runtime and engines.
//!
//! These messages define the communication protocol for engine interactions.
//! Messages are serialized as JSON for cross-process communication.

use crate::engine::EngineId;
use serde::{Deserialize, Serialize};

/// Request from runtime to engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineRequest {
    /// Initialize the engine
    Initialize {
        /// Configuration parameters
        config: serde_json::Value,
    },

    /// Shut down the engine
    Shutdown,

    /// Process audio for STT
    ProcessStt {
        /// Request ID for correlation
        request_id: u64,
        /// Audio samples (f32, mono, 16kHz typically)
        audio: Vec<f32>,
        /// Sample rate
        sample_rate: u32,
        /// Whether this is the final chunk
        is_final: bool,
    },

    /// Process text for translation
    ProcessTranslation {
        /// Request ID
        request_id: u64,
        /// Source text
        text: String,
        /// Source language
        source_lang: String,
        /// Target language
        target_lang: String,
    },

    /// Process text for TTS
    ProcessTts {
        /// Request ID
        request_id: u64,
        /// Text to synthesize
        text: String,
        /// Target language
        language: String,
        /// Optional voice ID
        voice_id: Option<String>,
    },

    /// Direct speech-to-speech processing
    ProcessS2s {
        /// Request ID
        request_id: u64,
        /// Audio samples
        audio: Vec<f32>,
        /// Sample rate
        sample_rate: u32,
        /// Source language
        source_lang: String,
        /// Target language
        target_lang: String,
        /// Whether this is the final chunk
        is_final: bool,
    },

    /// Ping for health check
    Ping {
        /// Timestamp for latency measurement
        timestamp_ms: u64,
    },

    /// Get engine capabilities
    GetCapabilities,

    /// Cancel a pending request
    Cancel {
        /// Request ID to cancel
        request_id: u64,
    },
}

/// Response from engine to runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineResponse {
    /// Engine initialized successfully
    Initialized {
        /// Engine ID
        engine_id: EngineId,
    },

    /// Engine shut down
    ShutdownComplete,

    /// STT result
    SttResult {
        /// Correlated request ID
        request_id: u64,
        /// Processing result
        result: ProcessingResult<SttOutput>,
    },

    /// Translation result
    TranslationResult {
        /// Correlated request ID
        request_id: u64,
        /// Processing result
        result: ProcessingResult<TranslationOutput>,
    },

    /// TTS result
    TtsResult {
        /// Correlated request ID
        request_id: u64,
        /// Processing result
        result: ProcessingResult<TtsOutput>,
    },

    /// Speech-to-speech result
    S2sResult {
        /// Correlated request ID
        request_id: u64,
        /// Processing result
        result: ProcessingResult<S2sOutput>,
    },

    /// Pong response
    Pong {
        /// Original timestamp
        timestamp_ms: u64,
        /// Engine timestamp
        engine_timestamp_ms: u64,
    },

    /// Capabilities response
    Capabilities {
        /// Engine info as JSON
        info: serde_json::Value,
    },

    /// Request cancelled
    Cancelled {
        /// Request ID that was cancelled
        request_id: u64,
    },

    /// Error response
    Error {
        /// Optional request ID if related to a specific request
        request_id: Option<u64>,
        /// Error code
        code: String,
        /// Error message
        message: String,
    },
}

/// Result of a processing operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProcessingResult<T> {
    /// Processing completed successfully
    Success {
        /// Output data
        data: T,
        /// Processing time in milliseconds
        processing_ms: u64,
    },
    /// Partial/streaming result
    Partial {
        /// Partial output data
        data: T,
        /// Whether more results are expected
        has_more: bool,
    },
    /// Processing failed
    Failed {
        /// Error code
        code: String,
        /// Error message
        message: String,
    },
}

impl<T> ProcessingResult<T> {
    /// Check if the result is successful.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Check if this is a partial result.
    pub fn is_partial(&self) -> bool {
        matches!(self, Self::Partial { .. })
    }

    /// Check if the result is a failure.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    /// Get the data if successful or partial.
    pub fn data(&self) -> Option<&T> {
        match self {
            Self::Success { data, .. } | Self::Partial { data, .. } => Some(data),
            Self::Failed { .. } => None,
        }
    }
}

/// STT output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttOutput {
    /// Transcribed text
    pub text: String,
    /// Detected language (if available)
    pub language: Option<String>,
    /// Confidence score (0.0 - 1.0)
    pub confidence: Option<f32>,
    /// Word-level timestamps if available
    pub words: Option<Vec<WordTiming>>,
}

/// Word timing information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTiming {
    /// The word
    pub word: String,
    /// Start time in milliseconds
    pub start_ms: u64,
    /// End time in milliseconds
    pub end_ms: u64,
    /// Confidence for this word
    pub confidence: Option<f32>,
}

/// Translation output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationOutput {
    /// Translated text
    pub text: String,
    /// Detected source language (if different from specified)
    pub detected_source: Option<String>,
}

/// TTS output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsOutput {
    /// Synthesized audio samples (f32)
    pub audio: Vec<f32>,
    /// Sample rate of the audio
    pub sample_rate: u32,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Speech-to-speech output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S2sOutput {
    /// Translated audio samples (f32)
    pub audio: Vec<f32>,
    /// Sample rate
    pub sample_rate: u32,
    /// Intermediate transcription (if available)
    pub transcription: Option<String>,
    /// Intermediate translation (if available)
    pub translation: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let request = EngineRequest::ProcessStt {
            request_id: 1,
            audio: vec![0.0, 0.1, 0.2],
            sample_rate: 16000,
            is_final: true,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("process_stt"));

        let parsed: EngineRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            EngineRequest::ProcessStt { request_id, .. } => assert_eq!(request_id, 1),
            _ => panic!("Wrong request type"),
        }
    }

    #[test]
    fn test_response_serialization() {
        let response = EngineResponse::SttResult {
            request_id: 1,
            result: ProcessingResult::Success {
                data: SttOutput {
                    text: "Hello world".to_string(),
                    language: Some("en".to_string()),
                    confidence: Some(0.95),
                    words: None,
                },
                processing_ms: 150,
            },
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("stt_result"));
        assert!(json.contains("Hello world"));
    }

    #[test]
    fn test_processing_result() {
        let result: ProcessingResult<String> = ProcessingResult::Success {
            data: "test".to_string(),
            processing_ms: 10,
        };
        assert!(result.is_success());
        assert_eq!(result.data(), Some(&"test".to_string()));

        let failed: ProcessingResult<String> = ProcessingResult::Failed {
            code: "ERR".to_string(),
            message: "failed".to_string(),
        };
        assert!(failed.is_failed());
        assert_eq!(failed.data(), None);
    }
}
