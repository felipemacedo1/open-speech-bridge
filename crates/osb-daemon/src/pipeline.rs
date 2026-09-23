//! Audio processing pipeline.
//!
//! The pipeline connects audio streams to processing engines.

use osb_core::audio::AudioFormat;
use tracing::info;

/// Pipeline stage identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// Capture from microphone
    Capture,
    /// Resample for engine input
    ResampleIn,
    /// Voice activity detection
    Vad,
    /// Speech-to-text
    Stt,
    /// Translation
    Translation,
    /// Text-to-speech
    Tts,
    /// Resample for output
    ResampleOut,
    /// Output to virtual device
    Output,
}

/// Pipeline configuration.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Enable outgoing translation (mic → translated virtual mic)
    pub enable_outgoing: bool,
    /// Enable incoming translation (app audio → translated headphones)
    pub enable_incoming: bool,
    /// Input audio format
    pub input_format: AudioFormat,
    /// Output audio format
    pub output_format: AudioFormat,
    /// Engine input format (typically 16kHz mono)
    pub engine_format: AudioFormat,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            enable_outgoing: true,
            enable_incoming: false,
            input_format: AudioFormat::DESKTOP_STANDARD,
            output_format: AudioFormat::DESKTOP_STANDARD,
            engine_format: AudioFormat::SPEECH_RECOGNITION,
        }
    }
}

/// The processing pipeline.
pub struct Pipeline {
    config: PipelineConfig,
    is_running: bool,
}

impl Pipeline {
    /// Create a new pipeline.
    pub fn new(config: PipelineConfig) -> Self {
        Self {
            config,
            is_running: false,
        }
    }

    /// Get the pipeline configuration.
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    /// Check if the pipeline is running.
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Start the pipeline.
    pub fn start(&mut self) -> Result<(), String> {
        if self.is_running {
            return Ok(());
        }

        info!("starting processing pipeline");

        // The pipeline flow for outgoing translation:
        // 1. Capture audio from physical mic
        // 2. Push to capture ring buffer
        // 3. Resample to engine format (48kHz→16kHz, stereo→mono)
        // 4. Run VAD to detect speech segments
        // 5. Send speech to STT engine
        // 6. Send text to translation engine
        // 7. Send translated text to TTS engine
        // 8. Resample TTS output to desktop format
        // 9. Push to virtual mic ring buffer
        // 10. Virtual mic serves to applications

        self.is_running = true;
        Ok(())
    }

    /// Stop the pipeline.
    pub fn stop(&mut self) -> Result<(), String> {
        if !self.is_running {
            return Ok(());
        }

        info!("stopping processing pipeline");
        self.is_running = false;
        Ok(())
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new(PipelineConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_lifecycle() {
        let mut pipeline = Pipeline::default();
        
        assert!(!pipeline.is_running());
        
        pipeline.start().unwrap();
        assert!(pipeline.is_running());
        
        pipeline.stop().unwrap();
        assert!(!pipeline.is_running());
    }
}
