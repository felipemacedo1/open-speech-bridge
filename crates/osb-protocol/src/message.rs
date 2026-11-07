//! Protocol messages between runtime and engines.
//!
//! These messages define the communication protocol for engine interactions.
//! Messages are serialized as JSON for cross-process communication.

use crate::engine::{EngineId, EngineInfo};
use serde::{Deserialize, Serialize};

/// Per-chunk audio stream metadata propagated across IPC boundaries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFrameMeta {
    /// Logical conversation/session identifier
    pub session_id: u64,
    /// Utterance identifier within the session
    pub utterance_id: u64,
    /// Monotonically increasing chunk counter within the utterance
    pub chunk_id: u64,
    /// Capture monotonic timestamp in microseconds
    pub timestamp_us: u64,
    /// Number of interleaved audio channels (typically 1 for mono engine input)
    pub channels: u16,
}

/// Request from runtime to engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineRequest {
    /// Versioned transport handshake and flow-control negotiation
    Hello {
        /// Runtime protocol version (SemVer)
        protocol_version: String,
        /// Maximum concurrent in-flight requests allowed by the runtime
        max_in_flight: u32,
        /// Maximum binary PCM payload size in bytes accepted per frame
        max_payload_bytes: u32,
    },

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

    /// Direct speech-to-text translation (e.g., PT-BR audio -> English text)
    ProcessSpeechTranslation {
        /// Request ID for correlation
        request_id: u64,
        /// Audio samples (f32, mono, 16kHz typically)
        audio: Vec<f32>,
        /// Sample rate in Hz
        sample_rate: u32,
        /// Source language code (e.g., "pt-BR")
        source_lang: String,
        /// Target language code (e.g., "en-US")
        target_lang: String,
        /// Whether this is the final chunk of the utterance
        is_final: bool,
        /// Optional stream/utterance timing metadata
        #[serde(default, skip_serializing_if = "Option::is_none")]
        meta: Option<AudioFrameMeta>,
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
    /// Handshake acknowledgement with worker metadata and negotiated limits
    Welcome {
        /// Worker protocol version (SemVer)
        protocol_version: String,
        /// Advertised engine capabilities and licensing metadata
        info: EngineInfo,
        /// Negotiated maximum concurrent in-flight requests
        max_in_flight: u32,
        /// Negotiated maximum binary PCM payload size in bytes
        max_payload_bytes: u32,
    },

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

    /// Direct speech-to-text translation result
    SpeechTranslationResult {
        /// Correlated request ID
        request_id: u64,
        /// Processing result
        result: ProcessingResult<SpeechTranslationOutput>,
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

/// Direct speech-to-text translation output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechTranslationOutput {
    /// Translated text in the target language
    pub text: String,
    /// Detected source language
    pub detected_source: Option<String>,
    /// Target language code
    pub target_lang: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: Option<f32>,
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

impl EngineRequest {
    /// Return the correlation request ID if this message carries one.
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::ProcessStt { request_id, .. }
            | Self::ProcessSpeechTranslation { request_id, .. }
            | Self::ProcessTranslation { request_id, .. }
            | Self::ProcessTts { request_id, .. }
            | Self::ProcessS2s { request_id, .. }
            | Self::Cancel { request_id } => Some(*request_id),
            Self::Hello { .. }
            | Self::Initialize { .. }
            | Self::Shutdown
            | Self::Ping { .. }
            | Self::GetCapabilities => None,
        }
    }
}

impl EngineResponse {
    /// Return the correlation request ID if this response resolves a specific request.
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::SttResult { request_id, .. }
            | Self::SpeechTranslationResult { request_id, .. }
            | Self::TranslationResult { request_id, .. }
            | Self::TtsResult { request_id, .. }
            | Self::S2sResult { request_id, .. }
            | Self::Cancelled { request_id } => Some(*request_id),
            Self::Error { request_id, .. } => *request_id,
            Self::Welcome { .. }
            | Self::Initialized { .. }
            | Self::ShutdownComplete
            | Self::Pong { .. }
            | Self::Capabilities { .. } => None,
        }
    }
}

/// Check SemVer compatibility between runtime and worker protocol versions.
///
/// For `0.y.z` pre-1.0 versions, `major` (`0`) and `minor` (`y`) must match.
/// For `>= 1.0.0`, `major` must match.
pub fn is_version_compatible(local: &str, remote: &str) -> bool {
    let parse_parts = |v: &str| -> Option<(u32, u32)> {
        let mut iter = v.trim().split('.');
        let major = iter.next()?.parse::<u32>().ok()?;
        let minor = iter.next()?.parse::<u32>().ok()?;
        Some((major, minor))
    };

    match (parse_parts(local), parse_parts(remote)) {
        (Some((0, l_minor)), Some((0, r_minor))) => l_minor == r_minor,
        (Some((l_major, _)), Some((r_major, _))) if l_major >= 1 => l_major == r_major,
        _ => false,
    }
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
