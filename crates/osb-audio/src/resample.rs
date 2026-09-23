//! Audio resampling utilities.
//!
//! This module provides sample rate conversion using the rubato library.
//! Resampling is needed at engine boundaries where models expect specific rates
//! (e.g., 16kHz for STT) but the audio system uses native rates (e.g., 48kHz).

use osb_core::audio::SampleRate;
use osb_core::error::{AudioError, Result};
use rubato::{FftFixedInOut, Resampler as RubatoResampler};
use tracing::debug;

/// Audio resampler for converting between sample rates.
pub struct Resampler {
    inner: FftFixedInOut<f32>,
    input_rate: SampleRate,
    output_rate: SampleRate,
    channels: usize,
}

impl Resampler {
    /// Create a new resampler.
    ///
    /// # Arguments
    ///
    /// * `input_rate` - Input sample rate
    /// * `output_rate` - Output sample rate
    /// * `channels` - Number of audio channels
    /// * `chunk_size` - Processing chunk size in frames
    ///
    /// # Errors
    ///
    /// Returns an error if the resampler cannot be initialized.
    pub fn new(
        input_rate: SampleRate,
        output_rate: SampleRate,
        channels: usize,
        chunk_size: usize,
    ) -> Result<Self> {
        let resampler = FftFixedInOut::new(
            input_rate.hz() as usize,
            output_rate.hz() as usize,
            chunk_size,
            channels,
        )
        .map_err(|e| AudioError::ResamplingError(e.to_string()))?;

        debug!(
            input_rate = input_rate.hz(),
            output_rate = output_rate.hz(),
            channels,
            chunk_size,
            "created resampler"
        );

        Ok(Self {
            inner: resampler,
            input_rate,
            output_rate,
            channels,
        })
    }

    /// Create a resampler for converting from desktop audio to speech model format.
    ///
    /// Converts from 48kHz stereo to 16kHz mono (common STT input format).
    pub fn desktop_to_speech(chunk_size: usize) -> Result<Self> {
        Self::new(SampleRate::PRO_48K, SampleRate::SPEECH_16K, 1, chunk_size)
    }

    /// Create a resampler for converting from speech model format to desktop audio.
    ///
    /// Converts from 16kHz mono to 48kHz stereo (for playback).
    pub fn speech_to_desktop(chunk_size: usize) -> Result<Self> {
        Self::new(SampleRate::SPEECH_16K, SampleRate::PRO_48K, 1, chunk_size)
    }

    /// Get the required input chunk size in frames.
    #[inline]
    pub fn input_frames_required(&self) -> usize {
        self.inner.input_frames_next()
    }

    /// Get the output chunk size in frames for a given input.
    #[inline]
    pub fn output_frames(&self) -> usize {
        self.inner.output_frames_next()
    }

    /// Get the input sample rate.
    #[inline]
    pub fn input_rate(&self) -> SampleRate {
        self.input_rate
    }

    /// Get the output sample rate.
    #[inline]
    pub fn output_rate(&self) -> SampleRate {
        self.output_rate
    }

    /// Get the number of channels.
    #[inline]
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Process a chunk of audio.
    ///
    /// # Arguments
    ///
    /// * `input` - Input samples, deinterleaved by channel
    /// * `output` - Output buffer, deinterleaved by channel
    ///
    /// # Returns
    ///
    /// Number of output frames produced.
    pub fn process(&mut self, input: &[Vec<f32>], output: &mut [Vec<f32>]) -> Result<usize> {
        let (_, frames) = self
            .inner
            .process_into_buffer(input, output, None)
            .map_err(|e| AudioError::ResamplingError(e.to_string()))?;
        Ok(frames)
    }

    /// Process interleaved audio samples.
    ///
    /// This is a convenience method that handles deinterleaving internally.
    ///
    /// # Arguments
    ///
    /// * `input` - Interleaved input samples
    ///
    /// # Returns
    ///
    /// Interleaved output samples.
    pub fn process_interleaved(&mut self, input: &[f32]) -> Result<Vec<f32>> {
        // Deinterleave input
        let frames = input.len() / self.channels;
        let mut input_channels: Vec<Vec<f32>> = (0..self.channels)
            .map(|_| Vec::with_capacity(frames))
            .collect();

        for (i, &sample) in input.iter().enumerate() {
            input_channels[i % self.channels].push(sample);
        }

        // Prepare output buffers
        let out_frames = self.output_frames();
        let mut output_channels: Vec<Vec<f32>> =
            (0..self.channels).map(|_| vec![0.0; out_frames]).collect();

        // Process
        let produced = self.process(&input_channels, &mut output_channels)?;

        // Interleave output
        let mut output = Vec::with_capacity(produced * self.channels);
        for frame in 0..produced {
            for channel in &output_channels {
                output.push(channel[frame]);
            }
        }

        Ok(output)
    }

    /// Reset the resampler state.
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

/// Calculate the resampling ratio between two sample rates.
#[inline]
pub fn resample_ratio(input_rate: SampleRate, output_rate: SampleRate) -> f64 {
    output_rate.hz() as f64 / input_rate.hz() as f64
}

/// Calculate the number of output frames for a given number of input frames.
#[inline]
pub fn output_frame_count(
    input_frames: usize,
    input_rate: SampleRate,
    output_rate: SampleRate,
) -> usize {
    let ratio = resample_ratio(input_rate, output_rate);
    (input_frames as f64 * ratio).ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_ratio() {
        let ratio = resample_ratio(SampleRate::PRO_48K, SampleRate::SPEECH_16K);
        assert!((ratio - (16000.0 / 48000.0)).abs() < 0.001);
    }

    #[test]
    fn test_output_frame_count() {
        // 48kHz -> 16kHz, 4800 frames (100ms at 48kHz) -> 1600 frames (100ms at 16kHz)
        let out = output_frame_count(4800, SampleRate::PRO_48K, SampleRate::SPEECH_16K);
        assert_eq!(out, 1600);
    }

    #[test]
    fn test_resampler_creation() {
        let resampler = Resampler::new(SampleRate::PRO_48K, SampleRate::SPEECH_16K, 1, 1024);
        assert!(resampler.is_ok());
    }

    #[test]
    fn test_resampler_process() {
        let mut resampler =
            Resampler::new(SampleRate::PRO_48K, SampleRate::SPEECH_16K, 1, 1024).unwrap();

        let input_frames = resampler.input_frames_required();
        let output_frames = resampler.output_frames();

        // Create test input (sine wave)
        let input: Vec<Vec<f32>> =
            vec![(0..input_frames).map(|i| (i as f32 * 0.01).sin()).collect()];

        let mut output: Vec<Vec<f32>> = vec![vec![0.0; output_frames]];

        let produced = resampler.process(&input, &mut output).unwrap();
        assert!(produced > 0);
        assert!(produced <= output_frames);
    }
}
