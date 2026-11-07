//! Bounded Voice Activity Detection (VAD) and utterance endpoint segmentation.
//!
//! Operates on mono `f32` PCM frames outside real-time audio callbacks, using
//! hysteresis RMS thresholds, a bounded pre-roll ring (to preserve initial
//! plosives), minimum voiced duration filtering, hangover silence endpointing,
//! and a strict maximum utterance length cap.

use std::collections::VecDeque;

/// Current activity state of the [`VadSegmenter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadState {
    /// Waiting for voiced energy above `start_rms_threshold`.
    Idle,
    /// Actively accumulating speech (including short intra-utterance pauses).
    InSpeech,
}

/// Configuration parameters for [`VadSegmenter`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadConfig {
    /// Expected mono input sample rate in Hz (default: `16_000`).
    pub sample_rate: u32,
    /// Analysis frame duration in milliseconds (default: `20` ms).
    pub frame_ms: u32,
    /// RMS threshold required to trigger speech onset from `Idle`.
    pub start_rms_threshold: f32,
    /// Lower hysteresis RMS threshold sustaining active speech.
    pub continue_rms_threshold: f32,
    /// Number of pre-speech frames retained in a bounded circular buffer.
    pub pre_roll_frames: usize,
    /// Minimum voiced duration in milliseconds required before emitting an utterance.
    pub min_speech_ms: u32,
    /// Trailing silence duration in milliseconds that closes an utterance.
    pub end_silence_ms: u32,
    /// Hard upper bound on utterance duration in milliseconds (prevents unbounded growth).
    pub max_utterance_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            frame_ms: 20,
            start_rms_threshold: 0.015,
            continue_rms_threshold: 0.008,
            pre_roll_frames: 5,
            min_speech_ms: 160,
            end_silence_ms: 360,
            max_utterance_ms: 15_000,
        }
    }
}

impl VadConfig {
    /// Samples per analysis frame.
    #[inline]
    pub fn frame_samples(&self) -> usize {
        ((u64::from(self.sample_rate) * u64::from(self.frame_ms)) / 1_000).max(1) as usize
    }

    /// Maximum samples allowed in one utterance buffer.
    #[inline]
    pub fn max_utterance_samples(&self) -> usize {
        ((u64::from(self.sample_rate) * u64::from(self.max_utterance_ms)) / 1_000)
            .max(self.frame_samples() as u64) as usize
    }
}

/// Completed mono speech segment emitted by [`VadSegmenter`].
#[derive(Debug, Clone)]
pub struct UtteranceSegment {
    /// Monotonically increasing utterance identifier (starting at `1`).
    pub utterance_id: u64,
    /// Contiguous mono `f32` PCM samples.
    pub samples: Vec<f32>,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Approximate start timestamp in microseconds.
    pub start_timestamp_us: u64,
    /// Approximate end timestamp in microseconds.
    pub end_timestamp_us: u64,
    /// `true` if closed due to `max_utterance_ms` cap or explicit stream flush.
    pub forced_cutoff: bool,
}

impl UtteranceSegment {
    /// Utterance duration in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        (self.samples.len() as u64 * 1_000) / u64::from(self.sample_rate)
    }
}

/// Stateful, bounded utterance segmenter driven by mono `f32` samples.
pub struct VadSegmenter {
    config: VadConfig,
    state: VadState,
    residual: Vec<f32>,
    pre_roll: VecDeque<Vec<f32>>,
    active_samples: Vec<f32>,
    voiced_frames: u32,
    trailing_silent_frames: u32,
    utterance_start_us: u64,
    stream_cursor_us: u64,
    next_utterance_id: u64,
}

impl VadSegmenter {
    /// Create a new segmenter with the given configuration.
    pub fn new(config: VadConfig) -> Self {
        let max_cap = config.max_utterance_samples();
        Self {
            pre_roll: VecDeque::with_capacity(config.pre_roll_frames.max(1)),
            residual: Vec::with_capacity(config.frame_samples()),
            active_samples: Vec::with_capacity(max_cap.min(config.sample_rate as usize * 4)),
            config,
            state: VadState::Idle,
            voiced_frames: 0,
            trailing_silent_frames: 0,
            utterance_start_us: 0,
            stream_cursor_us: 0,
            next_utterance_id: 1,
        }
    }

    /// Current detector state.
    pub fn state(&self) -> VadState {
        self.state
    }

    /// Segmenter configuration.
    pub fn config(&self) -> &VadConfig {
        &self.config
    }

    /// Feed mono `f32` samples and collect any completed utterances.
    pub fn push_samples(&mut self, samples: &[f32]) -> Vec<UtteranceSegment> {
        let mut completed = Vec::new();
        let frame_len = self.config.frame_samples();
        let mut offset = 0usize;

        if !self.residual.is_empty() {
            let needed = frame_len - self.residual.len();
            let take = needed.min(samples.len());
            self.residual.extend_from_slice(&samples[..take]);
            offset += take;

            if self.residual.len() == frame_len {
                let frame = std::mem::replace(&mut self.residual, Vec::with_capacity(frame_len));
                if let Some(seg) = self.process_frame(&frame) {
                    completed.push(seg);
                }
            }
        }

        while offset + frame_len <= samples.len() {
            let slice = &samples[offset..offset + frame_len];
            if let Some(seg) = self.process_frame(slice) {
                completed.push(seg);
            }
            offset += frame_len;
        }

        if offset < samples.len() {
            self.residual.extend_from_slice(&samples[offset..]);
        }

        completed
    }

    /// Flush any remaining in-progress speech at end-of-stream.
    pub fn flush(&mut self) -> Option<UtteranceSegment> {
        if !self.residual.is_empty() {
            let tail = std::mem::take(&mut self.residual);
            if self.state == VadState::InSpeech {
                let remaining_cap = self
                    .config
                    .max_utterance_samples()
                    .saturating_sub(self.active_samples.len());
                let to_copy = tail.len().min(remaining_cap);
                self.active_samples.extend_from_slice(&tail[..to_copy]);
                if frame_rms(&tail) >= self.config.continue_rms_threshold {
                    self.voiced_frames += 1;
                }
            } else if frame_rms(&tail) >= self.config.start_rms_threshold {
                self.active_samples.extend_from_slice(&tail);
                self.voiced_frames = 1;
                self.state = VadState::InSpeech;
            }
        }

        if self.state != VadState::InSpeech {
            self.reset_active();
            return None;
        }

        let min_voiced_frames = (self.config.min_speech_ms / self.config.frame_ms.max(1)).max(1);
        if self.voiced_frames < min_voiced_frames || self.active_samples.is_empty() {
            self.reset_active();
            return None;
        }

        Some(self.emit_active_utterance(true))
    }

    fn process_frame(&mut self, frame: &[f32]) -> Option<UtteranceSegment> {
        let frame_us = u64::from(self.config.frame_ms) * 1_000;
        let frame_start_us = self.stream_cursor_us;
        self.stream_cursor_us += frame_us;

        let rms = frame_rms(frame);

        match self.state {
            VadState::Idle => {
                if rms >= self.config.start_rms_threshold {
                    self.start_utterance_with_preroll(frame, frame_start_us);
                } else {
                    self.push_preroll(frame);
                }
                None
            }
            VadState::InSpeech => self.accumulate_speech_frame(frame, rms),
        }
    }

    fn push_preroll(&mut self, frame: &[f32]) {
        if self.config.pre_roll_frames == 0 {
            return;
        }
        if self.pre_roll.len() >= self.config.pre_roll_frames {
            self.pre_roll.pop_front();
        }
        self.pre_roll.push_back(frame.to_vec());
    }

    fn start_utterance_with_preroll(&mut self, onset_frame: &[f32], onset_us: u64) {
        let preroll_us = (self.pre_roll.len() as u64) * u64::from(self.config.frame_ms) * 1_000;
        self.utterance_start_us = onset_us.saturating_sub(preroll_us);
        self.active_samples.clear();

        let max_samples = self.config.max_utterance_samples();
        while let Some(pre) = self.pre_roll.pop_front() {
            let room = max_samples.saturating_sub(self.active_samples.len());
            self.active_samples
                .extend_from_slice(&pre[..pre.len().min(room)]);
        }

        let room = max_samples.saturating_sub(self.active_samples.len());
        self.active_samples
            .extend_from_slice(&onset_frame[..onset_frame.len().min(room)]);
        self.voiced_frames = 1;
        self.trailing_silent_frames = 0;
        self.state = VadState::InSpeech;
    }

    fn accumulate_speech_frame(&mut self, frame: &[f32], rms: f32) -> Option<UtteranceSegment> {
        let max_samples = self.config.max_utterance_samples();
        let room = max_samples.saturating_sub(self.active_samples.len());
        self.active_samples
            .extend_from_slice(&frame[..frame.len().min(room)]);

        if rms >= self.config.continue_rms_threshold {
            self.voiced_frames += 1;
            self.trailing_silent_frames = 0;
        } else {
            self.trailing_silent_frames += 1;
        }

        let end_silence_frames = (self.config.end_silence_ms / self.config.frame_ms.max(1)).max(1);
        let min_voiced_frames = (self.config.min_speech_ms / self.config.frame_ms.max(1)).max(1);

        if self.trailing_silent_frames >= end_silence_frames {
            if self.voiced_frames >= min_voiced_frames {
                return Some(self.emit_active_utterance(false));
            }
            self.reset_active();
            return None;
        }

        if self.active_samples.len() >= max_samples {
            if self.voiced_frames >= min_voiced_frames {
                return Some(self.emit_active_utterance(true));
            }
            self.reset_active();
        }

        None
    }

    fn emit_active_utterance(&mut self, forced_cutoff: bool) -> UtteranceSegment {
        let id = self.next_utterance_id;
        self.next_utterance_id += 1;

        let samples = std::mem::take(&mut self.active_samples);
        let start_us = self.utterance_start_us;
        let end_us = self.stream_cursor_us;
        self.reset_active();

        UtteranceSegment {
            utterance_id: id,
            samples,
            sample_rate: self.config.sample_rate,
            start_timestamp_us: start_us,
            end_timestamp_us: end_us,
            forced_cutoff,
        }
    }

    fn reset_active(&mut self) {
        self.active_samples.clear();
        self.pre_roll.clear();
        self.voiced_frames = 0;
        self.trailing_silent_frames = 0;
        self.state = VadState::Idle;
    }
}

impl Default for VadSegmenter {
    fn default() -> Self {
        Self::new(VadConfig::default())
    }
}

fn frame_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tone(samples: usize, amp: f32) -> Vec<f32> {
        (0..samples)
            .map(|i| ((i as f32) * 0.15).sin() * amp)
            .collect()
    }

    #[test]
    fn test_vad_ignores_pure_silence_and_short_clicks() {
        let mut vad = VadSegmenter::default();
        // 500 ms silence
        assert!(vad.push_samples(&vec![0.0; 8_000]).is_empty());
        // 40 ms click (< min_speech_ms = 160 ms) followed by 500 ms silence
        assert!(vad.push_samples(&make_tone(640, 0.2)).is_empty());
        assert!(vad.push_samples(&vec![0.0; 8_000]).is_empty());
        assert!(vad.flush().is_none());
    }

    #[test]
    fn test_vad_emits_utterance_with_preroll_after_hangover() {
        let mut vad = VadSegmenter::default();
        // 200 ms quiet lead-in (populates 5-frame = 100 ms pre-roll)
        assert!(vad.push_samples(&vec![0.001; 3_200]).is_empty());
        // 400 ms speech
        assert!(vad.push_samples(&make_tone(6_400, 0.25)).is_empty());
        assert_eq!(vad.state(), VadState::InSpeech);
        // 400 ms trailing silence (> 360 ms hangover) triggers endpoint
        let segments = vad.push_samples(&vec![0.0; 6_400]);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].utterance_id, 1);
        assert!(!segments[0].forced_cutoff);
        // 100ms pre-roll (1600) + 400ms speech (6400) + 360ms hangover (5760) = 13760 samples
        assert_eq!(segments[0].samples.len(), 13_760);
        assert_eq!(vad.state(), VadState::Idle);
    }

    #[test]
    fn test_vad_enforces_max_utterance_bound() {
        let cfg = VadConfig {
            max_utterance_ms: 500,
            min_speech_ms: 100,
            pre_roll_frames: 0,
            ..VadConfig::default()
        };
        let mut vad = VadSegmenter::new(cfg);
        // Push 1.2 seconds of continuous speech -> must force-cut into two 500 ms segments
        let segments = vad.push_samples(&make_tone(19_200, 0.25));
        assert_eq!(segments.len(), 2);
        assert!(segments[0].forced_cutoff);
        assert_eq!(segments[0].samples.len(), 8_000);
        assert!(segments[1].forced_cutoff);
        assert_eq!(segments[1].samples.len(), 8_000);
    }
}
