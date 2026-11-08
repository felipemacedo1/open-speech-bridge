//! Audio playback stream implementation.

use super::common::{
    build_audio_format_pod, f32_samples_to_bytes, setup_stop_timer, RT_BATCH_SIZE,
};
use super::context::PipeWireContext;
use crate::error::{PipeWireError, Result};
use osb_audio::buffer::{AudioRingBuffer, AudioRingConsumer, AudioRingProducer};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use pipewire as pw;
use pw::properties::properties;
use pw::spa::pod::Pod;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Audio playback stream for outputting translated audio to a local device.
pub struct PlaybackStream {
    stub_consumer: Option<AudioRingConsumer>,
    format: AudioFormat,
    device_id: String,
    is_running: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl PlaybackStream {
    /// Create a new playback stream for the specified device.
    pub fn new(
        _ctx: &PipeWireContext,
        device_id: &str,
        buffer_frames: u32,
    ) -> Result<(Self, AudioRingProducer)> {
        let format = AudioFormat {
            sample_rate: SampleRate::PRO_48K,
            sample_format: SampleFormat::F32,
            channels: ChannelLayout::Stereo,
        };

        let frames = if buffer_frames == 0 {
            4096
        } else {
            buffer_frames
        };
        let (producer, consumer) = AudioRingBuffer::new(frames, format.channels.channels());

        let stream = Self {
            stub_consumer: Some(consumer),
            format,
            device_id: device_id.to_string(),
            is_running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        };

        Ok((stream, producer))
    }

    /// Get the audio format of this stream.
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Get the device ID.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Check if the stream is currently playing.
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    /// Start real PipeWire playback in a dedicated thread.
    pub fn start(&mut self) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }

        let Some(consumer) = self.stub_consumer.take() else {
            warn!(device = %self.device_id, "playback stream consumer already moved");
            return Ok(());
        };

        info!(device = %self.device_id, "starting playback stream");
        let is_running = Arc::clone(&self.is_running);
        let device_id = self.device_id.clone();
        let format = self.format;

        let handle = std::thread::Builder::new()
            .name(format!("pw-playback-{}", self.device_id))
            .spawn(move || {
                if let Err(e) = run_playback_loop(consumer, &device_id, format, is_running) {
                    error!(device = %device_id, error = %e, "playback loop failed");
                }
            })
            .map_err(|e| {
                PipeWireError::Internal(format!("failed to spawn playback thread: {e}"))
            })?;

        self.thread_handle = Some(handle);
        self.is_running.store(true, Ordering::Release);
        Ok(())
    }

    /// Stop playback and join the PipeWire thread.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running() {
            return Ok(());
        }

        info!(device = %self.device_id, "stopping playback stream");
        self.is_running.store(false, Ordering::Release);

        if let Some(handle) = self.thread_handle.take() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = handle.join();
        }
        Ok(())
    }

    /// Read samples in stub mode (before `start()` is called).
    pub fn read_samples(&mut self, output: &mut [f32]) -> usize {
        if let Some(ref mut c) = self.stub_consumer {
            c.pop_or_silence(output)
        } else {
            output.fill(0.0);
            0
        }
    }

    /// Get current buffer occupancy in stub mode.
    pub fn buffer_occupancy(&self) -> f32 {
        self.stub_consumer
            .as_ref()
            .map(|c| c.occupancy().fill_ratio)
            .unwrap_or(0.0)
    }
}

impl Drop for PlaybackStream {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.stop();
        }
    }
}

struct PlaybackUserData {
    consumer: AudioRingConsumer,
}

fn run_playback_loop(
    consumer: AudioRingConsumer,
    device_id: &str,
    format: AudioFormat,
    is_running: Arc<AtomicBool>,
) -> Result<()> {
    pw::init();

    let channels = format.channels.channels() as usize;
    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .map_err(|e| PipeWireError::ConnectionFailed(format!("failed to create main loop: {e}")))?;
    let context = pw::context::ContextRc::new(&mainloop, None)
        .map_err(|e| PipeWireError::ConnectionFailed(format!("failed to create context: {e}")))?;
    let core = context.connect_rc(None).map_err(|e| {
        PipeWireError::ConnectionFailed(format!("failed to connect to PipeWire: {e}"))
    })?;

    let stream_props = if device_id.is_empty() || device_id == "default" {
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Communication",
            *pw::keys::NODE_NAME => "openspeechbridge-playback",
            *pw::keys::NODE_DESCRIPTION => "OpenSpeechBridge Playback",
        }
    } else {
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Communication",
            *pw::keys::NODE_NAME => "openspeechbridge-playback",
            *pw::keys::NODE_DESCRIPTION => "OpenSpeechBridge Playback",
            *pw::keys::TARGET_OBJECT => device_id,
        }
    };

    let stream = pw::stream::StreamRc::new(core.clone(), "osb-playback", stream_props)
        .map_err(|e| PipeWireError::StreamCreationFailed(format!("playback stream error: {e}")))?;

    let mainloop_weak = mainloop.downgrade();
    let _listener = stream
        .add_local_listener_with_user_data(PlaybackUserData { consumer })
        .state_changed(move |_stream, _ud, old, new| {
            debug!(?old, ?new, "playback stream state changed");
            if let pw::stream::StreamState::Error(err) = &new {
                error!(error = %err, "playback stream error");
                if let Some(ml) = mainloop_weak.upgrade() {
                    ml.quit();
                }
            }
        })
        .process(move |stream, user_data| {
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let requested_frames = buffer.requested() as usize;
                let datas = buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    if let Some(output) = data.data() {
                        let frame_stride = channels * 4;
                        let max_bytes = if requested_frames > 0 {
                            (requested_frames * frame_stride).min(output.len())
                        } else {
                            output.len()
                        };
                        let aligned_bytes = (max_bytes / frame_stride) * frame_stride;
                        let written = fill_playback_bytes(
                            &mut user_data.consumer,
                            &mut output[..aligned_bytes],
                        );
                        let chunk = data.chunk_mut();
                        *chunk.offset_mut() = 0;
                        *chunk.stride_mut() = frame_stride as i32;
                        *chunk.size_mut() = written as u32;
                    }
                }
            }
        })
        .register()
        .map_err(|e| PipeWireError::StreamCreationFailed(format!("listener error: {e}")))?;

    let values = build_audio_format_pod(format)?;
    let mut params = [Pod::from_bytes(&values)
        .ok_or_else(|| PipeWireError::Internal("failed to create format pod".to_string()))?];

    stream
        .connect(
            pw::spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| PipeWireError::StreamCreationFailed(format!("connect error: {e}")))?;

    let _timer = setup_stop_timer(&mainloop, is_running, 100)?;
    mainloop.run();
    let _ = stream.disconnect();
    Ok(())
}

#[inline]
fn fill_playback_bytes(consumer: &mut AudioRingConsumer, output: &mut [u8]) -> usize {
    let mut sample_buf = [0.0f32; RT_BATCH_SIZE];
    let mut byte_offset = 0usize;
    let total_samples = output.len() / 4;
    let mut samples_remaining = total_samples;

    while samples_remaining > 0 {
        let batch = samples_remaining.min(RT_BATCH_SIZE);
        let _ = consumer.pop_or_silence(&mut sample_buf[..batch]);
        let bytes_written = f32_samples_to_bytes(&sample_buf[..batch], &mut output[byte_offset..]);
        byte_offset += bytes_written;
        samples_remaining -= batch;
    }
    byte_offset
}

/// Builder for PlaybackStream.
pub struct PlaybackStreamBuilder {
    device_id: String,
    buffer_frames: u32,
    sample_rate: SampleRate,
    channels: ChannelLayout,
}

impl PlaybackStreamBuilder {
    /// Create a new builder.
    pub fn new(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            buffer_frames: 4096,
            sample_rate: SampleRate::PRO_48K,
            channels: ChannelLayout::Stereo,
        }
    }

    /// Set buffer size.
    pub fn buffer_frames(mut self, frames: u32) -> Self {
        self.buffer_frames = frames;
        self
    }

    /// Set sample rate.
    pub fn sample_rate(mut self, rate: SampleRate) -> Self {
        self.sample_rate = rate;
        self
    }

    /// Set channels.
    pub fn channels(mut self, channels: ChannelLayout) -> Self {
        self.channels = channels;
        self
    }

    /// Build the stream.
    pub fn build(self, ctx: &PipeWireContext) -> Result<(PlaybackStream, AudioRingProducer)> {
        PlaybackStream::new(ctx, &self.device_id, self.buffer_frames)
    }
}
