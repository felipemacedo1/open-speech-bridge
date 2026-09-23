//! Engine capability definitions.
//!
//! Capabilities describe what an engine can do. The runtime uses these
//! to select appropriate engines for a given task.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A language pair for translation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LanguagePair {
    /// Source language code (e.g., "pt-BR", "en-US")
    pub source: String,
    /// Target language code
    pub target: String,
}

impl LanguagePair {
    /// Create a new language pair.
    pub fn new(source: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
        }
    }

    /// Portuguese to English.
    pub fn pt_en() -> Self {
        Self::new("pt-BR", "en-US")
    }

    /// English to Portuguese.
    pub fn en_pt() -> Self {
        Self::new("en-US", "pt-BR")
    }
}

/// Engine capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    // Speech Recognition
    /// Speech-to-text (batch mode)
    Stt,
    /// Streaming speech-to-text
    StreamingStt,
    /// Voice activity detection
    Vad,

    // Translation
    /// Text translation (batch mode)
    TextTranslation,
    /// Streaming text translation
    StreamingTranslation,

    // Speech Synthesis
    /// Text-to-speech (batch mode)
    Tts,
    /// Streaming text-to-speech
    StreamingTts,

    // Speech-to-Speech
    /// Direct speech-to-speech translation
    SpeechToSpeech,
    /// Streaming speech-to-speech translation
    StreamingSpeechToSpeech,

    // Voice Processing
    /// Speaker diarization
    Diarization,
    /// Speaker embedding extraction
    SpeakerEmbedding,
    /// Voice cloning / conditioning
    VoiceConditioning,

    // Hardware
    /// GPU acceleration support
    Gpu,
    /// CPU-only operation
    CpuOnly,
}

/// A set of capabilities with associated metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySet {
    /// Supported capabilities
    capabilities: HashSet<Capability>,
    /// Supported language pairs for translation
    language_pairs: Vec<LanguagePair>,
    /// Supported languages for STT
    stt_languages: Vec<String>,
    /// Supported languages for TTS
    tts_languages: Vec<String>,
}

impl CapabilitySet {
    /// Create an empty capability set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a capability.
    pub fn add(&mut self, capability: Capability) -> &mut Self {
        self.capabilities.insert(capability);
        self
    }

    /// Add multiple capabilities.
    pub fn add_all(&mut self, capabilities: impl IntoIterator<Item = Capability>) -> &mut Self {
        self.capabilities.extend(capabilities);
        self
    }

    /// Add a supported language pair.
    pub fn add_language_pair(&mut self, pair: LanguagePair) -> &mut Self {
        self.language_pairs.push(pair);
        self
    }

    /// Add a supported STT language.
    pub fn add_stt_language(&mut self, lang: impl Into<String>) -> &mut Self {
        self.stt_languages.push(lang.into());
        self
    }

    /// Add a supported TTS language.
    pub fn add_tts_language(&mut self, lang: impl Into<String>) -> &mut Self {
        self.tts_languages.push(lang.into());
        self
    }

    /// Check if a capability is supported.
    pub fn has(&self, capability: &Capability) -> bool {
        self.capabilities.contains(capability)
    }

    /// Check if all specified capabilities are supported.
    pub fn has_all(&self, capabilities: &[Capability]) -> bool {
        capabilities.iter().all(|c| self.capabilities.contains(c))
    }

    /// Check if any of the specified capabilities are supported.
    pub fn has_any(&self, capabilities: &[Capability]) -> bool {
        capabilities.iter().any(|c| self.capabilities.contains(c))
    }

    /// Check if a language pair is supported for translation.
    pub fn supports_translation(&self, source: &str, target: &str) -> bool {
        self.language_pairs.iter().any(|p| p.source == source && p.target == target)
    }

    /// Check if a language is supported for STT.
    pub fn supports_stt_language(&self, lang: &str) -> bool {
        self.stt_languages.iter().any(|l| l == lang || l.starts_with(&format!("{}-", lang.split('-').next().unwrap_or(lang))))
    }

    /// Check if a language is supported for TTS.
    pub fn supports_tts_language(&self, lang: &str) -> bool {
        self.tts_languages.iter().any(|l| l == lang || l.starts_with(&format!("{}-", lang.split('-').next().unwrap_or(lang))))
    }

    /// Get all capabilities.
    pub fn capabilities(&self) -> &HashSet<Capability> {
        &self.capabilities
    }

    /// Get supported language pairs.
    pub fn language_pairs(&self) -> &[LanguagePair] {
        &self.language_pairs
    }

    /// Get supported STT languages.
    pub fn stt_languages(&self) -> &[String] {
        &self.stt_languages
    }

    /// Get supported TTS languages.
    pub fn tts_languages(&self) -> &[String] {
        &self.tts_languages
    }

    /// Check if the engine can perform streaming STT.
    pub fn can_stream_stt(&self) -> bool {
        self.has(&Capability::StreamingStt)
    }

    /// Check if the engine can perform streaming translation.
    pub fn can_stream_translation(&self) -> bool {
        self.has(&Capability::StreamingTranslation)
    }

    /// Check if the engine can perform streaming TTS.
    pub fn can_stream_tts(&self) -> bool {
        self.has(&Capability::StreamingTts)
    }

    /// Check if the engine can perform direct speech-to-speech.
    pub fn can_speech_to_speech(&self) -> bool {
        self.has(&Capability::SpeechToSpeech) || self.has(&Capability::StreamingSpeechToSpeech)
    }

    /// Check if GPU acceleration is available.
    pub fn has_gpu(&self) -> bool {
        self.has(&Capability::Gpu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_set() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::StreamingStt)
            .add(Capability::Gpu)
            .add_stt_language("pt-BR")
            .add_stt_language("en-US");

        assert!(caps.has(&Capability::StreamingStt));
        assert!(caps.has(&Capability::Gpu));
        assert!(!caps.has(&Capability::Tts));
        assert!(caps.can_stream_stt());
        assert!(caps.has_gpu());
    }

    #[test]
    fn test_language_support() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::TextTranslation)
            .add_language_pair(LanguagePair::pt_en())
            .add_language_pair(LanguagePair::en_pt());

        assert!(caps.supports_translation("pt-BR", "en-US"));
        assert!(caps.supports_translation("en-US", "pt-BR"));
        assert!(!caps.supports_translation("fr-FR", "en-US"));
    }

    #[test]
    fn test_has_all_has_any() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::Stt)
            .add(Capability::Tts);

        assert!(caps.has_all(&[Capability::Stt, Capability::Tts]));
        assert!(!caps.has_all(&[Capability::Stt, Capability::Gpu]));
        assert!(caps.has_any(&[Capability::Stt, Capability::Gpu]));
        assert!(!caps.has_any(&[Capability::Gpu, Capability::Diarization]));
    }
}
