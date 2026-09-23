//! Sample format conversion utilities.
//!
//! This module provides conversion between different sample formats
//! (i16, i32, f32, f64) and channel layouts (mono, stereo).

use osb_core::audio::{ChannelLayout, SampleFormat};

/// Sample format converter.
pub struct SampleConverter;

impl SampleConverter {
    /// Convert i16 samples to f32.
    #[inline]
    pub fn i16_to_f32(input: &[i16], output: &mut [f32]) {
        const SCALE: f32 = 1.0 / 32768.0;
        for (i, &sample) in input.iter().enumerate() {
            if i < output.len() {
                output[i] = sample as f32 * SCALE;
            }
        }
    }

    /// Convert f32 samples to i16.
    #[inline]
    pub fn f32_to_i16(input: &[f32], output: &mut [i16]) {
        const SCALE: f32 = 32767.0;
        for (i, &sample) in input.iter().enumerate() {
            if i < output.len() {
                // Clamp to [-1, 1] then scale
                let clamped = sample.clamp(-1.0, 1.0);
                output[i] = (clamped * SCALE) as i16;
            }
        }
    }

    /// Convert i32 samples to f32.
    #[inline]
    pub fn i32_to_f32(input: &[i32], output: &mut [f32]) {
        const SCALE: f32 = 1.0 / 2147483648.0;
        for (i, &sample) in input.iter().enumerate() {
            if i < output.len() {
                output[i] = sample as f32 * SCALE;
            }
        }
    }

    /// Convert f32 samples to i32.
    #[inline]
    pub fn f32_to_i32(input: &[f32], output: &mut [i32]) {
        const SCALE: f32 = 2147483647.0;
        for (i, &sample) in input.iter().enumerate() {
            if i < output.len() {
                let clamped = sample.clamp(-1.0, 1.0);
                output[i] = (clamped * SCALE) as i32;
            }
        }
    }

    /// Convert stereo to mono by averaging channels.
    #[inline]
    pub fn stereo_to_mono(input: &[f32], output: &mut [f32]) {
        for (i, chunk) in input.chunks_exact(2).enumerate() {
            if i < output.len() {
                output[i] = (chunk[0] + chunk[1]) * 0.5;
            }
        }
    }

    /// Convert mono to stereo by duplicating samples.
    #[inline]
    pub fn mono_to_stereo(input: &[f32], output: &mut [f32]) {
        for (i, &sample) in input.iter().enumerate() {
            let out_idx = i * 2;
            if out_idx + 1 < output.len() {
                output[out_idx] = sample;
                output[out_idx + 1] = sample;
            }
        }
    }
}

/// In-place sample conversion trait.
pub trait ConvertSamples {
    /// Convert samples in place from one format to another.
    fn convert_format(&mut self, from: SampleFormat, to: SampleFormat);
    
    /// Convert channel layout in place.
    fn convert_channels(&mut self, from: ChannelLayout, to: ChannelLayout);
}

impl ConvertSamples for Vec<f32> {
    fn convert_format(&mut self, from: SampleFormat, to: SampleFormat) {
        // f32 to f32 is a no-op
        if from == to || (from == SampleFormat::F32 && to == SampleFormat::F32) {
            // No conversion needed
        }
        // For other conversions, we'd need type changes which Vec<f32> can't do in place
        // This is primarily for documentation; actual cross-type conversion needs separate buffers
    }

    fn convert_channels(&mut self, from: ChannelLayout, to: ChannelLayout) {
        match (from, to) {
            (ChannelLayout::Stereo, ChannelLayout::Mono) => {
                let mono_len = self.len() / 2;
                let mut mono = vec![0.0f32; mono_len];
                SampleConverter::stereo_to_mono(self, &mut mono);
                *self = mono;
            }
            (ChannelLayout::Mono, ChannelLayout::Stereo) => {
                let stereo_len = self.len() * 2;
                let mut stereo = vec![0.0f32; stereo_len];
                SampleConverter::mono_to_stereo(self, &mut stereo);
                *self = stereo;
            }
            _ => {} // Same layout, no conversion needed
        }
    }
}

/// Interleave separate channel buffers into a single interleaved buffer.
pub fn interleave(channels: &[&[f32]], output: &mut [f32]) {
    let num_channels = channels.len();
    if num_channels == 0 {
        return;
    }
    
    let frames = channels[0].len();
    for frame in 0..frames {
        for (ch, channel) in channels.iter().enumerate() {
            let out_idx = frame * num_channels + ch;
            if out_idx < output.len() && frame < channel.len() {
                output[out_idx] = channel[frame];
            }
        }
    }
}

/// Deinterleave an interleaved buffer into separate channel buffers.
pub fn deinterleave(input: &[f32], num_channels: usize, outputs: &mut [Vec<f32>]) {
    if num_channels == 0 || outputs.len() < num_channels {
        return;
    }
    
    let frames = input.len() / num_channels;
    for output in outputs.iter_mut().take(num_channels) {
        output.clear();
        output.reserve(frames);
    }
    
    for (i, &sample) in input.iter().enumerate() {
        let ch = i % num_channels;
        outputs[ch].push(sample);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i16_f32_roundtrip() {
        let input_i16: Vec<i16> = vec![-32768, -16384, 0, 16383, 32767];
        let mut f32_buf = vec![0.0f32; 5];
        let mut output_i16 = vec![0i16; 5];

        SampleConverter::i16_to_f32(&input_i16, &mut f32_buf);
        SampleConverter::f32_to_i16(&f32_buf, &mut output_i16);

        // Allow for some quantization error
        for (orig, converted) in input_i16.iter().zip(output_i16.iter()) {
            assert!((orig - converted).abs() <= 1);
        }
    }

    #[test]
    fn test_stereo_to_mono() {
        let stereo = vec![0.5f32, 0.5, 1.0, 0.0, -0.5, 0.5];
        let mut mono = vec![0.0f32; 3];

        SampleConverter::stereo_to_mono(&stereo, &mut mono);

        assert!((mono[0] - 0.5).abs() < 0.001);
        assert!((mono[1] - 0.5).abs() < 0.001);
        assert!((mono[2] - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_mono_to_stereo() {
        let mono = vec![0.5f32, 1.0, -0.5];
        let mut stereo = vec![0.0f32; 6];

        SampleConverter::mono_to_stereo(&mono, &mut stereo);

        assert_eq!(stereo, vec![0.5, 0.5, 1.0, 1.0, -0.5, -0.5]);
    }

    #[test]
    fn test_interleave_deinterleave() {
        let left = vec![1.0f32, 2.0, 3.0];
        let right = vec![4.0f32, 5.0, 6.0];
        let mut interleaved = vec![0.0f32; 6];

        interleave(&[&left, &right], &mut interleaved);
        assert_eq!(interleaved, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);

        let mut outputs = vec![Vec::new(), Vec::new()];
        deinterleave(&interleaved, 2, &mut outputs);
        assert_eq!(outputs[0], left);
        assert_eq!(outputs[1], right);
    }
}
