//! Audio resampling utilities.
//!
//! This module provides sample rate conversion using the rubato library.
//! Resampling is needed at engine boundaries where models expect specific rates
//! (e.g., 16kHz for STT) but the audio system uses native rates (e.g., 48kHz).

use osb_core::audio::SampleRate;
use osb_core::error::{AudioError, Result};
use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler as RubatoResampler};
use tracing::debug;

/// Audio resampler for converting between sample rates.
pub struct Resampler {
    inner: Fft<f32>,
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
        let resampler = Fft::new(
            input_rate.hz() as usize,
            output_rate.hz() as usize,
            chunk_size,
            channels,
            FixedSync::Both,
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
        let to_err = |e: &dyn std::fmt::Display| AudioError::ResamplingError(e.to_string());
        let in_frames = input.iter().map(Vec::len).min().unwrap_or(0);
        let out_frames = output.iter().map(Vec::len).min().unwrap_or(0);
        let in_adapter =
            SequentialSliceOfVecs::new(input, self.channels, in_frames).map_err(|e| to_err(&e))?;
        let mut out_adapter = SequentialSliceOfVecs::new_mut(output, self.channels, out_frames)
            .map_err(|e| to_err(&e))?;
        let (_, frames) = self
            .inner
            .process_into_buffer(&in_adapter, &mut out_adapter, None)
            .map_err(|e| to_err(&e))?;
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

/// Stateful mono resampler accepting arbitrary chunk sizes without fixed-frame constraints.
pub struct StreamingMonoResampler {
    inner: Option<Resampler>,
    input_rate: SampleRate,
    output_rate: SampleRate,
    residual: Vec<f32>,
}

impl StreamingMonoResampler {
    /// Create a streaming mono resampler between `input_rate` and `output_rate`.
    ///
    /// When `input_rate == output_rate`, operates as a zero-overhead passthrough.
    pub fn new(input_rate: SampleRate, output_rate: SampleRate, chunk_size: usize) -> Result<Self> {
        if input_rate == output_rate {
            return Ok(Self {
                inner: None,
                input_rate,
                output_rate,
                residual: Vec::new(),
            });
        }

        let resampler = Resampler::new(input_rate, output_rate, 1, chunk_size.max(256))?;
        let req = resampler.input_frames_required();
        Ok(Self {
            inner: Some(resampler),
            input_rate,
            output_rate,
            residual: Vec::with_capacity(req * 2),
        })
    }

    /// Feed an arbitrary slice of mono `f32` samples and emit available resampled samples.
    pub fn push(&mut self, samples: &[f32]) -> Result<Vec<f32>> {
        let Some(ref mut resampler) = self.inner else {
            return Ok(samples.to_vec());
        };

        self.residual.extend_from_slice(samples);
        let mut out = Vec::new();

        loop {
            let req = resampler.input_frames_required();
            if self.residual.len() < req {
                break;
            }
            let chunk: Vec<f32> = self.residual.drain(..req).collect();
            let produced = resampler.process_interleaved(&chunk)?;
            out.extend_from_slice(&produced);
        }

        Ok(out)
    }

    /// Flush any buffered tail samples by zero-padding the final block and trimming output.
    pub fn flush(&mut self) -> Result<Vec<f32>> {
        let Some(ref mut resampler) = self.inner else {
            return Ok(std::mem::take(&mut self.residual));
        };

        if self.residual.is_empty() {
            return Ok(Vec::new());
        }

        let tail_len = self.residual.len();
        let expected_out = output_frame_count(tail_len, self.input_rate, self.output_rate);
        let req = resampler.input_frames_required();

        let mut padded = std::mem::take(&mut self.residual);
        padded.resize(req, 0.0);
        let mut produced = resampler.process_interleaved(&padded)?;
        produced.truncate(expected_out);
        resampler.reset();
        Ok(produced)
    }
}

/// Resample a complete mono `f32` buffer of arbitrary length from `input_rate` to `output_rate`.
pub fn resample_mono_buffer(
    samples: &[f32],
    input_rate: SampleRate,
    output_rate: SampleRate,
) -> Result<Vec<f32>> {
    if samples.is_empty() || input_rate == output_rate {
        return Ok(samples.to_vec());
    }

    let expected_total = output_frame_count(samples.len(), input_rate, output_rate);
    let mut streamer = StreamingMonoResampler::new(input_rate, output_rate, 1024)?;
    let mut out = streamer.push(samples)?;
    let tail = streamer.flush()?;
    out.extend_from_slice(&tail);
    out.truncate(expected_total);
    Ok(out)
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
    fn test_resample_ratio_upsampling() {
        let ratio = resample_ratio(SampleRate::SPEECH_16K, SampleRate::PRO_48K);
        assert!((ratio - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_resample_ratio_same_rate() {
        let ratio = resample_ratio(SampleRate::PRO_48K, SampleRate::PRO_48K);
        assert!((ratio - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_output_frame_count() {
        // 48kHz -> 16kHz, 4800 frames (100ms at 48kHz) -> 1600 frames (100ms at 16kHz)
        let out = output_frame_count(4800, SampleRate::PRO_48K, SampleRate::SPEECH_16K);
        assert_eq!(out, 1600);
    }

    #[test]
    fn test_output_frame_count_upsampling() {
        // 16kHz -> 48kHz, 1600 frames -> 4800 frames
        let out = output_frame_count(1600, SampleRate::SPEECH_16K, SampleRate::PRO_48K);
        assert_eq!(out, 4800);
    }

    #[test]
    fn test_output_frame_count_zero() {
        let out = output_frame_count(0, SampleRate::PRO_48K, SampleRate::SPEECH_16K);
        assert_eq!(out, 0);
    }

    #[test]
    fn test_resampler_creation() {
        let resampler = Resampler::new(SampleRate::PRO_48K, SampleRate::SPEECH_16K, 1, 1024);
        assert!(resampler.is_ok());
    }

    #[test]
    fn test_resampler_desktop_to_speech() {
        let resampler = Resampler::desktop_to_speech(1024);
        assert!(resampler.is_ok());

        let r = resampler.unwrap();
        assert_eq!(r.input_rate(), SampleRate::PRO_48K);
        assert_eq!(r.output_rate(), SampleRate::SPEECH_16K);
        assert_eq!(r.channels(), 1);
    }

    #[test]
    fn test_resampler_speech_to_desktop() {
        let resampler = Resampler::speech_to_desktop(1024);
        assert!(resampler.is_ok());

        let r = resampler.unwrap();
        assert_eq!(r.input_rate(), SampleRate::SPEECH_16K);
        assert_eq!(r.output_rate(), SampleRate::PRO_48K);
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

    #[test]
    fn test_resampler_reset() {
        let mut resampler =
            Resampler::new(SampleRate::PRO_48K, SampleRate::SPEECH_16K, 1, 1024).unwrap();

        // Process some data
        let input_frames = resampler.input_frames_required();
        let output_frames = resampler.output_frames();
        let input: Vec<Vec<f32>> = vec![vec![0.5; input_frames]];
        let mut output: Vec<Vec<f32>> = vec![vec![0.0; output_frames]];
        let _ = resampler.process(&input, &mut output);

        // Reset and verify it doesn't panic
        resampler.reset();

        // Process again after reset
        let produced = resampler.process(&input, &mut output).unwrap();
        assert!(produced > 0);
    }

    #[test]
    fn test_resampler_stereo() {
        let resampler = Resampler::new(SampleRate::PRO_48K, SampleRate::SPEECH_16K, 2, 1024);
        assert!(resampler.is_ok());

        let r = resampler.unwrap();
        assert_eq!(r.channels(), 2);
    }

    #[test]
    fn test_resampler_process_interleaved() {
        let mut resampler =
            Resampler::new(SampleRate::PRO_48K, SampleRate::SPEECH_16K, 1, 1024).unwrap();

        let input_frames = resampler.input_frames_required();

        // Create interleaved input (mono, so same as non-interleaved)
        let input: Vec<f32> = (0..input_frames).map(|i| (i as f32 * 0.01).sin()).collect();

        let output = resampler.process_interleaved(&input).unwrap();
        assert!(!output.is_empty());
    }

    #[test]
    fn test_resample_mono_buffer_arbitrary_length_24k_to_48k() {
        // Odd length (e.g. 3777 samples @ 24kHz -> 7554 samples @ 48kHz)
        let input: Vec<f32> = (0..3_777).map(|i| (i as f32 * 0.02).sin() * 0.4).collect();
        let out = resample_mono_buffer(&input, SampleRate::TTS_24K, SampleRate::PRO_48K).unwrap();
        assert_eq!(out.len(), 7_554);
    }
}
