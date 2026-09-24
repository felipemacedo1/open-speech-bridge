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
///
/// # Arguments
///
/// * `channels` - Slice of channel buffers (e.g., `[&left, &right]`)
/// * `output` - Output buffer for interleaved samples
///
/// # Example
///
/// ```
/// use osb_audio::convert::interleave;
///
/// let left = [1.0f32, 2.0, 3.0];
/// let right = [4.0f32, 5.0, 6.0];
/// let mut interleaved = [0.0f32; 6];
///
/// interleave(&[&left, &right], &mut interleaved);
/// assert_eq!(interleaved, [1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
/// ```
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
///
/// # Arguments
///
/// * `input` - Interleaved sample buffer
/// * `num_channels` - Number of channels to extract
/// * `outputs` - Output vectors for each channel (must have at least `num_channels` elements)
///
/// # Example
///
/// ```
/// use osb_audio::convert::deinterleave;
///
/// let interleaved = [1.0f32, 4.0, 2.0, 5.0, 3.0, 6.0];
/// let mut outputs = vec![Vec::new(), Vec::new()];
///
/// deinterleave(&interleaved, 2, &mut outputs);
/// assert_eq!(outputs[0], vec![1.0, 2.0, 3.0]); // left
/// assert_eq!(outputs[1], vec![4.0, 5.0, 6.0]); // right
/// ```
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
    fn test_i32_f32_roundtrip() {
        let input_i32: Vec<i32> = vec![-2147483648, -1073741824, 0, 1073741823, 2147483647];
        let mut f32_buf = vec![0.0f32; 5];
        let mut output_i32 = vec![0i32; 5];

        SampleConverter::i32_to_f32(&input_i32, &mut f32_buf);
        SampleConverter::f32_to_i32(&f32_buf, &mut output_i32);

        // Check that values are in reasonable range (f32 precision limits exact roundtrip)
        for (orig, converted) in input_i32.iter().zip(output_i32.iter()) {
            let diff = (*orig as i64 - *converted as i64).abs();
            assert!(diff < 1000); // Allow for f32 precision loss
        }
    }

    #[test]
    fn test_f32_to_i16_clamping() {
        // Test that values outside [-1, 1] are clamped
        let input = vec![-2.0f32, -1.5, 1.5, 2.0];
        let mut output = vec![0i16; 4];

        SampleConverter::f32_to_i16(&input, &mut output);

        assert_eq!(output[0], -32767); // Clamped to -1.0
        assert_eq!(output[1], -32767); // Clamped to -1.0
        assert_eq!(output[2], 32767); // Clamped to 1.0
        assert_eq!(output[3], 32767); // Clamped to 1.0
    }

    #[test]
    fn test_f32_to_i32_clamping() {
        let input = vec![-2.0f32, 2.0];
        let mut output = vec![0i32; 2];

        SampleConverter::f32_to_i32(&input, &mut output);

        // Values are clamped to [-1, 1] then scaled by 2147483647
        // -1.0 * 2147483647 = -2147483647, but cast to i32 may give -2147483648
        assert!(output[0] <= -2147483647); // Negative clamped value
        assert_eq!(output[1], 2147483647); // 1.0 * 2147483647
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
    fn test_stereo_to_mono_empty() {
        let stereo: Vec<f32> = vec![];
        let mut mono = vec![0.0f32; 0];

        SampleConverter::stereo_to_mono(&stereo, &mut mono);
        assert!(mono.is_empty());
    }

    #[test]
    fn test_mono_to_stereo_empty() {
        let mono: Vec<f32> = vec![];
        let mut stereo = vec![0.0f32; 0];

        SampleConverter::mono_to_stereo(&mono, &mut stereo);
        assert!(stereo.is_empty());
    }

    #[test]
    fn test_convert_channels_stereo_to_mono() {
        let mut samples = vec![1.0f32, 0.0, 0.5, 0.5, -1.0, 1.0];
        samples.convert_channels(ChannelLayout::Stereo, ChannelLayout::Mono);

        assert_eq!(samples.len(), 3);
        assert!((samples[0] - 0.5).abs() < 0.001);
        assert!((samples[1] - 0.5).abs() < 0.001);
        assert!((samples[2] - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_convert_channels_mono_to_stereo() {
        let mut samples = vec![0.5f32, 1.0, -0.5];
        samples.convert_channels(ChannelLayout::Mono, ChannelLayout::Stereo);

        assert_eq!(samples.len(), 6);
        assert_eq!(samples, vec![0.5, 0.5, 1.0, 1.0, -0.5, -0.5]);
    }

    #[test]
    fn test_convert_channels_same_layout() {
        let mut samples = vec![1.0f32, 2.0, 3.0];
        let original = samples.clone();
        samples.convert_channels(ChannelLayout::Mono, ChannelLayout::Mono);
        assert_eq!(samples, original);
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

    #[test]
    fn test_interleave_empty() {
        let channels: Vec<&[f32]> = vec![];
        let mut output = vec![0.0f32; 0];

        interleave(&channels, &mut output);
        assert!(output.is_empty());
    }

    #[test]
    fn test_deinterleave_empty() {
        let input: Vec<f32> = vec![];
        let mut outputs = vec![Vec::new(), Vec::new()];

        deinterleave(&input, 2, &mut outputs);
        assert!(outputs[0].is_empty());
        assert!(outputs[1].is_empty());
    }

    #[test]
    fn test_deinterleave_zero_channels() {
        let input = vec![1.0f32, 2.0, 3.0];
        let mut outputs: Vec<Vec<f32>> = vec![];

        deinterleave(&input, 0, &mut outputs);
        // Should not panic, just do nothing
    }

    #[test]
    fn test_interleave_three_channels() {
        let ch1 = vec![1.0f32, 4.0];
        let ch2 = vec![2.0f32, 5.0];
        let ch3 = vec![3.0f32, 6.0];
        let mut interleaved = vec![0.0f32; 6];

        interleave(&[&ch1, &ch2, &ch3], &mut interleaved);
        assert_eq!(interleaved, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn test_conversion_with_mismatched_buffer_sizes() {
        // Input larger than output - should only fill output
        let input_i16: Vec<i16> = vec![1000, 2000, 3000, 4000, 5000];
        let mut f32_buf = vec![0.0f32; 3];

        SampleConverter::i16_to_f32(&input_i16, &mut f32_buf);

        // Only first 3 should be converted
        assert!(f32_buf[0] > 0.0);
        assert!(f32_buf[1] > 0.0);
        assert!(f32_buf[2] > 0.0);
    }
}
