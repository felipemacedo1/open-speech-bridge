//! Audio format specifications and utilities.
//!
//! This module defines the core audio types used throughout OpenSpeechBridge.
//! These types describe audio formats at transport and processing boundaries.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Sample rate specification.
///
/// Common rates used in speech processing:
/// - 16000 Hz: Standard for most STT models (Whisper, etc.)
/// - 22050 Hz: Common for TTS output
/// - 44100 Hz: CD quality, common desktop default
/// - 48000 Hz: Professional audio, PipeWire default
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SampleRate(pub u32);

impl SampleRate {
    /// 16 kHz - Standard for speech recognition models
    pub const SPEECH_16K: Self = Self(16000);
    /// 22.05 kHz - Common TTS output rate
    pub const TTS_22K: Self = Self(22050);
    /// 44.1 kHz - CD quality
    pub const CD_44K: Self = Self(44100);
    /// 48 kHz - Professional audio / PipeWire default
    pub const PRO_48K: Self = Self(48000);

    /// Create a new sample rate.
    #[inline]
    pub const fn new(rate: u32) -> Self {
        Self(rate)
    }

    /// Get the rate in Hz.
    #[inline]
    pub const fn hz(self) -> u32 {
        self.0
    }

    /// Calculate the number of samples for a given duration in milliseconds.
    #[inline]
    pub const fn samples_for_ms(self, ms: u32) -> u32 {
        self.0 * ms / 1000
    }

    /// Calculate duration in milliseconds for a given number of samples.
    #[inline]
    pub const fn ms_for_samples(self, samples: u32) -> u32 {
        samples * 1000 / self.0
    }
}

impl Default for SampleRate {
    fn default() -> Self {
        Self::PRO_48K
    }
}

impl fmt::Display for SampleRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} Hz", self.0)
    }
}

/// Sample format specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleFormat {
    /// 16-bit signed integer (little-endian)
    I16,
    /// 32-bit signed integer (little-endian)
    I32,
    /// 32-bit floating point [-1.0, 1.0]
    F32,
    /// 64-bit floating point [-1.0, 1.0]
    F64,
}

impl SampleFormat {
    /// Size of a single sample in bytes.
    #[inline]
    pub const fn bytes_per_sample(self) -> usize {
        match self {
            Self::I16 => 2,
            Self::I32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    /// Whether this format is floating point.
    #[inline]
    pub const fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
}

impl Default for SampleFormat {
    fn default() -> Self {
        Self::F32
    }
}

impl fmt::Display for SampleFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::I16 => write!(f, "i16"),
            Self::I32 => write!(f, "i32"),
            Self::F32 => write!(f, "f32"),
            Self::F64 => write!(f, "f64"),
        }
    }
}

/// Channel layout specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelLayout {
    /// Single channel (mono)
    Mono,
    /// Two channels (stereo)
    Stereo,
}

impl ChannelLayout {
    /// Number of channels.
    #[inline]
    pub const fn channels(self) -> u8 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
        }
    }
}

impl Default for ChannelLayout {
    fn default() -> Self {
        Self::Stereo
    }
}

impl fmt::Display for ChannelLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mono => write!(f, "mono"),
            Self::Stereo => write!(f, "stereo"),
        }
    }
}

/// Complete audio format specification.
///
/// Describes the format of audio data at a specific point in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AudioFormat {
    /// Sample rate
    pub sample_rate: SampleRate,
    /// Sample format
    pub sample_format: SampleFormat,
    /// Channel layout
    pub channels: ChannelLayout,
}

impl AudioFormat {
    /// Standard format for speech recognition models (16kHz mono f32).
    pub const SPEECH_RECOGNITION: Self = Self {
        sample_rate: SampleRate::SPEECH_16K,
        sample_format: SampleFormat::F32,
        channels: ChannelLayout::Mono,
    };

    /// Standard desktop audio format (48kHz stereo f32).
    pub const DESKTOP_STANDARD: Self = Self {
        sample_rate: SampleRate::PRO_48K,
        sample_format: SampleFormat::F32,
        channels: ChannelLayout::Stereo,
    };

    /// Create a new audio format specification.
    pub const fn new(
        sample_rate: SampleRate,
        sample_format: SampleFormat,
        channels: ChannelLayout,
    ) -> Self {
        Self {
            sample_rate,
            sample_format,
            channels,
        }
    }

    /// Bytes per frame (sample × channels).
    #[inline]
    pub const fn bytes_per_frame(&self) -> usize {
        self.sample_format.bytes_per_sample() * self.channels.channels() as usize
    }

    /// Calculate buffer size in bytes for a given duration in milliseconds.
    #[inline]
    pub const fn buffer_size_for_ms(&self, ms: u32) -> usize {
        let frames = self.sample_rate.samples_for_ms(ms);
        frames as usize * self.bytes_per_frame()
    }

    /// Check if this format requires conversion to another format.
    #[inline]
    pub const fn requires_conversion(&self, target: &AudioFormat) -> bool {
        self.sample_rate.0 != target.sample_rate.0
            || !matches!(
                (&self.sample_format, &target.sample_format),
                (SampleFormat::I16, SampleFormat::I16)
                    | (SampleFormat::I32, SampleFormat::I32)
                    | (SampleFormat::F32, SampleFormat::F32)
                    | (SampleFormat::F64, SampleFormat::F64)
            )
            || self.channels.channels() != target.channels.channels()
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::DESKTOP_STANDARD
    }
}

impl fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.sample_rate, self.sample_format, self.channels
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_rate_calculations() {
        let rate = SampleRate::SPEECH_16K;
        assert_eq!(rate.samples_for_ms(1000), 16000);
        assert_eq!(rate.samples_for_ms(100), 1600);
        assert_eq!(rate.samples_for_ms(10), 160);
        assert_eq!(rate.ms_for_samples(16000), 1000);
    }

    #[test]
    fn test_audio_format_buffer_size() {
        let format = AudioFormat::SPEECH_RECOGNITION;
        // 10ms of 16kHz mono f32 = 160 samples × 4 bytes = 640 bytes
        assert_eq!(format.buffer_size_for_ms(10), 640);

        let stereo = AudioFormat::DESKTOP_STANDARD;
        // 10ms of 48kHz stereo f32 = 480 samples × 8 bytes = 3840 bytes
        assert_eq!(stereo.buffer_size_for_ms(10), 3840);
    }

    #[test]
    fn test_format_requires_conversion() {
        let speech = AudioFormat::SPEECH_RECOGNITION;
        let desktop = AudioFormat::DESKTOP_STANDARD;
        assert!(desktop.requires_conversion(&speech));
        assert!(!speech.requires_conversion(&speech));
    }
}
