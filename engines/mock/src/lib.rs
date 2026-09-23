//! Mock Engine for OpenSpeechBridge
//!
//! A testing engine that provides basic audio transformation without ML models.
//! Useful for testing the engine protocol and pipeline without downloading models.

use osb_protocol::capability::{Capability, CapabilitySet};
use osb_protocol::engine::{EngineId, EngineInfo, EngineType};
use osb_protocol::message::{
    EngineRequest, EngineResponse, ProcessingResult, SttOutput, TranslationOutput, TtsOutput,
};
use std::time::Instant;

/// Mock engine implementation.
pub struct MockEngine {
    info: EngineInfo,
}

impl MockEngine {
    /// Create a new mock engine.
    pub fn new() -> Self {
        let mut capabilities = CapabilitySet::new();
        capabilities
            .add(Capability::Stt)
            .add(Capability::StreamingStt)
            .add(Capability::TextTranslation)
            .add(Capability::Tts)
            .add(Capability::CpuOnly)
            .add_stt_language("en-US")
            .add_stt_language("pt-BR")
            .add_tts_language("en-US")
            .add_tts_language("pt-BR")
            .add_language_pair(osb_protocol::capability::LanguagePair::pt_en())
            .add_language_pair(osb_protocol::capability::LanguagePair::en_pt());

        let info = EngineInfo::builder("mock")
            .name("Mock Engine")
            .version("0.1.0")
            .engine_type(EngineType::Mock)
            .capabilities(capabilities)
            .license("Apache-2.0")
            .commercial_use(true)
            .build();

        Self { info }
    }

    /// Get engine info.
    pub fn info(&self) -> &EngineInfo {
        &self.info
    }

    /// Handle a request and produce a response.
    pub fn handle_request(&self, request: EngineRequest) -> EngineResponse {
        match request {
            EngineRequest::Initialize { .. } => EngineResponse::Initialized {
                engine_id: self.info.id.clone(),
            },

            EngineRequest::Shutdown => EngineResponse::ShutdownComplete,

            EngineRequest::GetCapabilities => EngineResponse::Capabilities {
                info: serde_json::to_value(&self.info).unwrap_or_default(),
            },

            EngineRequest::Ping { timestamp_ms } => EngineResponse::Pong {
                timestamp_ms,
                engine_timestamp_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
            },

            EngineRequest::ProcessStt {
                request_id,
                audio,
                sample_rate: _,
                is_final,
            } => {
                let start = Instant::now();
                
                // Mock: return "detected speech" if audio has energy
                let energy: f32 = audio.iter().map(|s| s.abs()).sum::<f32>() / audio.len() as f32;
                let text = if energy > 0.01 {
                    "[mock: speech detected]".to_string()
                } else {
                    "[mock: silence]".to_string()
                };

                let result = if is_final {
                    ProcessingResult::Success {
                        data: SttOutput {
                            text,
                            language: Some("en".to_string()),
                            confidence: Some(0.95),
                            words: None,
                        },
                        processing_ms: start.elapsed().as_millis() as u64,
                    }
                } else {
                    ProcessingResult::Partial {
                        data: SttOutput {
                            text,
                            language: None,
                            confidence: None,
                            words: None,
                        },
                        has_more: true,
                    }
                };

                EngineResponse::SttResult { request_id, result }
            }

            EngineRequest::ProcessTranslation {
                request_id,
                text,
                source_lang,
                target_lang,
            } => {
                let start = Instant::now();
                
                // Mock: just add prefix
                let translated = format!("[{} → {}] {}", source_lang, target_lang, text);

                EngineResponse::TranslationResult {
                    request_id,
                    result: ProcessingResult::Success {
                        data: TranslationOutput {
                            text: translated,
                            detected_source: Some(source_lang),
                        },
                        processing_ms: start.elapsed().as_millis() as u64,
                    },
                }
            }

            EngineRequest::ProcessTts {
                request_id,
                text,
                language: _,
                voice_id: _,
            } => {
                let start = Instant::now();
                
                // Mock: generate silence with length proportional to text
                let duration_ms = (text.len() as u64 * 50).min(5000);
                let sample_rate = 22050u32;
                let num_samples = (sample_rate as u64 * duration_ms / 1000) as usize;
                
                // Generate a simple tone instead of silence for testing
                let audio: Vec<f32> = (0..num_samples)
                    .map(|i| {
                        let t = i as f32 / sample_rate as f32;
                        (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.3
                    })
                    .collect();

                EngineResponse::TtsResult {
                    request_id,
                    result: ProcessingResult::Success {
                        data: TtsOutput {
                            audio,
                            sample_rate,
                            duration_ms,
                        },
                        processing_ms: start.elapsed().as_millis() as u64,
                    },
                }
            }

            EngineRequest::ProcessS2s { request_id, .. } => EngineResponse::Error {
                request_id: Some(request_id),
                code: "NOT_IMPLEMENTED".to_string(),
                message: "S2S not implemented in mock engine".to_string(),
            },

            EngineRequest::Cancel { request_id } => EngineResponse::Cancelled { request_id },
        }
    }
}

impl Default for MockEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_engine_creation() {
        let engine = MockEngine::new();
        assert_eq!(engine.info().id.as_str(), "mock");
        assert_eq!(engine.info().name, "Mock Engine");
    }

    #[test]
    fn test_mock_stt() {
        let engine = MockEngine::new();
        
        let request = EngineRequest::ProcessStt {
            request_id: 1,
            audio: vec![0.5; 1000],
            sample_rate: 16000,
            is_final: true,
        };
        
        let response = engine.handle_request(request);
        
        match response {
            EngineResponse::SttResult { request_id, result } => {
                assert_eq!(request_id, 1);
                assert!(result.is_success());
            }
            _ => panic!("Expected SttResult"),
        }
    }

    #[test]
    fn test_mock_translation() {
        let engine = MockEngine::new();
        
        let request = EngineRequest::ProcessTranslation {
            request_id: 2,
            text: "Hello".to_string(),
            source_lang: "en".to_string(),
            target_lang: "pt".to_string(),
        };
        
        let response = engine.handle_request(request);
        
        match response {
            EngineResponse::TranslationResult { request_id, result } => {
                assert_eq!(request_id, 2);
                if let ProcessingResult::Success { data, .. } = result {
                    assert!(data.text.contains("Hello"));
                }
            }
            _ => panic!("Expected TranslationResult"),
        }
    }

    #[test]
    fn test_mock_tts() {
        let engine = MockEngine::new();
        
        let request = EngineRequest::ProcessTts {
            request_id: 3,
            text: "Test".to_string(),
            language: "en".to_string(),
            voice_id: None,
        };
        
        let response = engine.handle_request(request);
        
        match response {
            EngineResponse::TtsResult { request_id, result } => {
                assert_eq!(request_id, 3);
                if let ProcessingResult::Success { data, .. } = result {
                    assert!(!data.audio.is_empty());
                    assert_eq!(data.sample_rate, 22050);
                }
            }
            _ => panic!("Expected TtsResult"),
        }
    }
}
