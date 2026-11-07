//! Bounded binary wire framing (`OSBP` v1) for local engine IPC.
//!
//! Each message on a Unix Domain Socket (or byte stream) is framed with a fixed
//! 16-byte little-endian prefix followed by a UTF-8 JSON control header and an
//! optional raw `f32` little-endian (`f32le`) PCM payload:
//!
//! ```text
//! +-------------------+-------------------+-------------------+-------------------+
//! | magic: "OSBP" (4) | version: u16 (2)  | flags: u16 (2)    | header_len: u32   |
//! +-------------------+-------------------+-------------------+-------------------+
//! | payload_len: u32  | JSON control header (header_len B)    | f32le PCM (N*4 B) |
//! +-------------------+---------------------------------------+-------------------+
//! ```
//!
//! Audio-carrying messages strip their `Vec<f32>` samples out of the JSON
//! control header and transmit them in the contiguous `f32le` binary payload,
//! avoiding float-to-ASCII bloat and enabling zero-copy NumPy/Rust views.

use crate::message::{
    AudioFrameMeta, EngineRequest, EngineResponse, ProcessingResult, S2sOutput, TtsOutput,
};
use serde::Serialize;
use std::io::{Read, Write};
use thiserror::Error;

/// Four-byte magic prefix identifying an OpenSpeechBridge Protocol frame.
pub const WIRE_MAGIC: [u8; 4] = *b"OSBP";

/// Binary wire framing format version.
pub const WIRE_VERSION: u16 = 1;

/// Fixed byte size of the binary frame prefix.
pub const FRAME_PREFIX_LEN: usize = 16;

/// Default upper bound for JSON control headers (64 KiB).
pub const DEFAULT_MAX_HEADER_BYTES: u32 = 64 * 1024;

/// Default upper bound for binary `f32le` PCM payloads (4 MiB = ~65.5 s @ 16 kHz mono).
pub const DEFAULT_MAX_PAYLOAD_BYTES: u32 = 4 * 1024 * 1024;

/// Bitflags attached to a binary wire frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FrameFlags(u16);

impl FrameFlags {
    /// No flags set.
    pub const NONE: Self = Self(0);
    /// Frame carries raw `f32le` PCM samples in the binary payload section.
    pub const HAS_PCM_PAYLOAD: Self = Self(1 << 0);
    /// Marks start of detected speech (`VAD` start).
    pub const SPEECH_START: Self = Self(1 << 1);
    /// Marks end of detected speech (`VAD` endpoint).
    pub const SPEECH_END: Self = Self(1 << 2);
    /// Marks final chunk of an utterance.
    pub const END_OF_UTTERANCE: Self = Self(1 << 3);
    /// Marks end of session stream.
    pub const END_OF_STREAM: Self = Self(1 << 4);
    /// Indicates upstream capture ring overflow / sample discontinuity prior to this frame.
    pub const DISCONTINUITY: Self = Self(1 << 5);

    /// Construct flags from raw `u16` bits.
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// Return raw `u16` bits.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Check whether all bits in `other` are set in `self`.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Combine two flag sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Size limits enforced before allocating frame buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLimits {
    /// Maximum allowed JSON control header size in bytes.
    pub max_header_bytes: u32,
    /// Maximum allowed binary PCM payload size in bytes.
    pub max_payload_bytes: u32,
}

impl Default for FrameLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            max_payload_bytes: DEFAULT_MAX_PAYLOAD_BYTES,
        }
    }
}

/// Decoded 16-byte frame prefix validated against [`FrameLimits`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePrefix {
    /// Frame bitflags.
    pub flags: FrameFlags,
    /// Validated JSON header length in bytes.
    pub header_len: u32,
    /// Validated binary payload length in bytes.
    pub payload_len: u32,
}

/// Errors produced when encoding or decoding wire frames.
#[derive(Debug, Error)]
pub enum FramingError {
    /// Underlying stream I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON control header serialization/deserialization failure.
    #[error("JSON control header error: {0}")]
    Json(#[from] serde_json::Error),

    /// Stream closed cleanly or mid-frame before prefix/body completed.
    #[error("unexpected end of stream while reading frame")]
    UnexpectedEof,

    /// Invalid frame magic bytes.
    #[error("invalid frame magic: {0:?}")]
    InvalidMagic([u8; 4]),

    /// Unsupported binary wire framing version.
    #[error("unsupported wire frame version: {0}")]
    UnsupportedVersion(u16),

    /// JSON header exceeds configured maximum.
    #[error("control header size {actual} exceeds limit {max} bytes")]
    HeaderTooLarge {
        /// Observed size in bytes.
        actual: u32,
        /// Configured limit in bytes.
        max: u32,
    },

    /// Binary payload exceeds configured maximum.
    #[error("PCM payload size {actual} exceeds limit {max} bytes")]
    PayloadTooLarge {
        /// Observed size in bytes.
        actual: u32,
        /// Configured limit in bytes.
        max: u32,
    },

    /// Binary `f32le` payload byte length is not a multiple of 4.
    #[error("unaligned f32le PCM payload size: {byte_len} bytes")]
    UnalignedPcmPayload {
        /// Observed byte length.
        byte_len: u32,
    },
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum BorrowedAudioRequestHeader<'a> {
    #[serde(rename = "process_stt")]
    Stt {
        request_id: u64,
        audio: &'static [f32],
        sample_rate: u32,
        is_final: bool,
    },
    #[serde(rename = "process_speech_translation")]
    SpeechTranslation {
        request_id: u64,
        audio: &'static [f32],
        sample_rate: u32,
        source_lang: &'a str,
        target_lang: &'a str,
        is_final: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        meta: &'a Option<AudioFrameMeta>,
    },
    #[serde(rename = "process_s2s")]
    S2s {
        request_id: u64,
        audio: &'static [f32],
        sample_rate: u32,
        source_lang: &'a str,
        target_lang: &'a str,
        is_final: bool,
    },
}

#[derive(Serialize)]
struct BorrowedTtsData {
    audio: &'static [f32],
    sample_rate: u32,
    duration_ms: u64,
}

#[derive(Serialize)]
struct BorrowedS2sData<'a> {
    audio: &'static [f32],
    sample_rate: u32,
    transcription: &'a Option<String>,
    translation: &'a Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum BorrowedProcessingResult<T> {
    Success { data: T, processing_ms: u64 },
    Partial { data: T, has_more: bool },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum BorrowedAudioResponseHeader<'a> {
    TtsResult {
        request_id: u64,
        result: BorrowedProcessingResult<BorrowedTtsData>,
    },
    S2sResult {
        request_id: u64,
        result: BorrowedProcessingResult<BorrowedS2sData<'a>>,
    },
}

/// Parse and validate a 16-byte wire prefix against `limits` prior to allocation.
pub fn parse_frame_prefix(
    prefix_bytes: &[u8; FRAME_PREFIX_LEN],
    limits: FrameLimits,
) -> Result<FramePrefix, FramingError> {
    let magic = [
        prefix_bytes[0],
        prefix_bytes[1],
        prefix_bytes[2],
        prefix_bytes[3],
    ];
    if magic != WIRE_MAGIC {
        return Err(FramingError::InvalidMagic(magic));
    }

    let version = u16::from_le_bytes([prefix_bytes[4], prefix_bytes[5]]);
    if version != WIRE_VERSION {
        return Err(FramingError::UnsupportedVersion(version));
    }

    let flags = FrameFlags::from_bits(u16::from_le_bytes([prefix_bytes[6], prefix_bytes[7]]));
    let header_len = u32::from_le_bytes([
        prefix_bytes[8],
        prefix_bytes[9],
        prefix_bytes[10],
        prefix_bytes[11],
    ]);
    let payload_len = u32::from_le_bytes([
        prefix_bytes[12],
        prefix_bytes[13],
        prefix_bytes[14],
        prefix_bytes[15],
    ]);

    if header_len > limits.max_header_bytes {
        return Err(FramingError::HeaderTooLarge {
            actual: header_len,
            max: limits.max_header_bytes,
        });
    }
    if payload_len > limits.max_payload_bytes {
        return Err(FramingError::PayloadTooLarge {
            actual: payload_len,
            max: limits.max_payload_bytes,
        });
    }
    if (flags.contains(FrameFlags::HAS_PCM_PAYLOAD) || payload_len > 0) && payload_len % 4 != 0 {
        return Err(FramingError::UnalignedPcmPayload {
            byte_len: payload_len,
        });
    }

    Ok(FramePrefix {
        flags,
        header_len,
        payload_len,
    })
}

fn assemble_frame(
    header_json: &[u8],
    pcm_samples: Option<&[f32]>,
    mut flags: FrameFlags,
    limits: FrameLimits,
) -> Result<Vec<u8>, FramingError> {
    let header_len =
        u32::try_from(header_json.len()).map_err(|_| FramingError::HeaderTooLarge {
            actual: u32::MAX,
            max: limits.max_header_bytes,
        })?;
    if header_len > limits.max_header_bytes {
        return Err(FramingError::HeaderTooLarge {
            actual: header_len,
            max: limits.max_header_bytes,
        });
    }

    let samples = pcm_samples.unwrap_or(&[]);
    let payload_bytes_usize =
        samples
            .len()
            .checked_mul(4)
            .ok_or(FramingError::PayloadTooLarge {
                actual: u32::MAX,
                max: limits.max_payload_bytes,
            })?;
    let payload_len =
        u32::try_from(payload_bytes_usize).map_err(|_| FramingError::PayloadTooLarge {
            actual: u32::MAX,
            max: limits.max_payload_bytes,
        })?;
    if payload_len > limits.max_payload_bytes {
        return Err(FramingError::PayloadTooLarge {
            actual: payload_len,
            max: limits.max_payload_bytes,
        });
    }

    if pcm_samples.is_some() {
        flags = flags.union(FrameFlags::HAS_PCM_PAYLOAD);
    }

    let mut buf = Vec::with_capacity(FRAME_PREFIX_LEN + header_json.len() + payload_bytes_usize);
    buf.extend_from_slice(&WIRE_MAGIC);
    buf.extend_from_slice(&WIRE_VERSION.to_le_bytes());
    buf.extend_from_slice(&flags.bits().to_le_bytes());
    buf.extend_from_slice(&header_len.to_le_bytes());
    buf.extend_from_slice(&payload_len.to_le_bytes());
    buf.extend_from_slice(header_json);
    for &sample in samples {
        buf.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(buf)
}

fn decode_pcm_bytes(payload_bytes: &[u8]) -> Result<Vec<f32>, FramingError> {
    if payload_bytes.len() % 4 != 0 {
        return Err(FramingError::UnalignedPcmPayload {
            byte_len: payload_bytes.len() as u32,
        });
    }
    let mut samples = Vec::with_capacity(payload_bytes.len() / 4);
    for chunk in payload_bytes.chunks_exact(4) {
        samples.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(samples)
}

/// Encode an [`EngineRequest`] into a bounded binary `OSBP` v1 frame.
pub fn encode_request_frame(
    request: &EngineRequest,
    flags: FrameFlags,
    limits: FrameLimits,
) -> Result<Vec<u8>, FramingError> {
    match request {
        EngineRequest::ProcessStt {
            request_id,
            audio,
            sample_rate,
            is_final,
        } => {
            let hdr = BorrowedAudioRequestHeader::Stt {
                request_id: *request_id,
                audio: &[],
                sample_rate: *sample_rate,
                is_final: *is_final,
            };
            let header_json = serde_json::to_vec(&hdr)?;
            let extra = if *is_final {
                flags.union(FrameFlags::END_OF_UTTERANCE)
            } else {
                flags
            };
            assemble_frame(&header_json, Some(audio), extra, limits)
        }
        EngineRequest::ProcessSpeechTranslation {
            request_id,
            audio,
            sample_rate,
            source_lang,
            target_lang,
            is_final,
            meta,
        } => {
            let hdr = BorrowedAudioRequestHeader::SpeechTranslation {
                request_id: *request_id,
                audio: &[],
                sample_rate: *sample_rate,
                source_lang,
                target_lang,
                is_final: *is_final,
                meta,
            };
            let header_json = serde_json::to_vec(&hdr)?;
            let extra = if *is_final {
                flags.union(FrameFlags::END_OF_UTTERANCE)
            } else {
                flags
            };
            assemble_frame(&header_json, Some(audio), extra, limits)
        }
        EngineRequest::ProcessS2s {
            request_id,
            audio,
            sample_rate,
            source_lang,
            target_lang,
            is_final,
        } => {
            let hdr = BorrowedAudioRequestHeader::S2s {
                request_id: *request_id,
                audio: &[],
                sample_rate: *sample_rate,
                source_lang,
                target_lang,
                is_final: *is_final,
            };
            let header_json = serde_json::to_vec(&hdr)?;
            let extra = if *is_final {
                flags.union(FrameFlags::END_OF_UTTERANCE)
            } else {
                flags
            };
            assemble_frame(&header_json, Some(audio), extra, limits)
        }
        _ => {
            let header_json = serde_json::to_vec(request)?;
            assemble_frame(&header_json, None, flags, limits)
        }
    }
}

/// Rehydrate an [`EngineRequest`] from its validated prefix, JSON header, and PCM payload bytes.
pub fn rehydrate_request_from_parts(
    prefix: FramePrefix,
    header_bytes: &[u8],
    payload_bytes: &[u8],
) -> Result<(EngineRequest, FrameFlags), FramingError> {
    let mut request: EngineRequest = serde_json::from_slice(header_bytes)?;
    if prefix.flags.contains(FrameFlags::HAS_PCM_PAYLOAD) || !payload_bytes.is_empty() {
        let pcm = decode_pcm_bytes(payload_bytes)?;
        match &mut request {
            EngineRequest::ProcessStt { audio, .. }
            | EngineRequest::ProcessSpeechTranslation { audio, .. }
            | EngineRequest::ProcessS2s { audio, .. } => {
                *audio = pcm;
            }
            _ => {}
        }
    }
    Ok((request, prefix.flags))
}

/// Decode a complete byte slice containing one `OSBP` v1 request frame.
pub fn decode_request_frame(
    frame_bytes: &[u8],
    limits: FrameLimits,
) -> Result<(EngineRequest, FrameFlags), FramingError> {
    if frame_bytes.len() < FRAME_PREFIX_LEN {
        return Err(FramingError::UnexpectedEof);
    }
    let mut prefix_arr = [0u8; FRAME_PREFIX_LEN];
    prefix_arr.copy_from_slice(&frame_bytes[..FRAME_PREFIX_LEN]);
    let prefix = parse_frame_prefix(&prefix_arr, limits)?;

    let body_end = FRAME_PREFIX_LEN + prefix.header_len as usize + prefix.payload_len as usize;
    if frame_bytes.len() < body_end {
        return Err(FramingError::UnexpectedEof);
    }

    let header_end = FRAME_PREFIX_LEN + prefix.header_len as usize;
    rehydrate_request_from_parts(
        prefix,
        &frame_bytes[FRAME_PREFIX_LEN..header_end],
        &frame_bytes[header_end..body_end],
    )
}

/// Encode an [`EngineResponse`] into a bounded binary `OSBP` v1 frame.
pub fn encode_response_frame(
    response: &EngineResponse,
    flags: FrameFlags,
    limits: FrameLimits,
) -> Result<Vec<u8>, FramingError> {
    match response {
        EngineResponse::TtsResult { request_id, result } => {
            encode_tts_response(*request_id, result, response, flags, limits)
        }
        EngineResponse::S2sResult { request_id, result } => {
            encode_s2s_response(*request_id, result, response, flags, limits)
        }
        _ => {
            let header_json = serde_json::to_vec(response)?;
            assemble_frame(&header_json, None, flags, limits)
        }
    }
}

fn encode_tts_response(
    request_id: u64,
    result: &ProcessingResult<TtsOutput>,
    full_response: &EngineResponse,
    flags: FrameFlags,
    limits: FrameLimits,
) -> Result<Vec<u8>, FramingError> {
    match result {
        ProcessingResult::Success {
            data,
            processing_ms,
        } => {
            let hdr = BorrowedAudioResponseHeader::TtsResult {
                request_id,
                result: BorrowedProcessingResult::Success {
                    data: BorrowedTtsData {
                        audio: &[],
                        sample_rate: data.sample_rate,
                        duration_ms: data.duration_ms,
                    },
                    processing_ms: *processing_ms,
                },
            };
            let header_json = serde_json::to_vec(&hdr)?;
            assemble_frame(&header_json, Some(&data.audio), flags, limits)
        }
        ProcessingResult::Partial { data, has_more } => {
            let hdr = BorrowedAudioResponseHeader::TtsResult {
                request_id,
                result: BorrowedProcessingResult::Partial {
                    data: BorrowedTtsData {
                        audio: &[],
                        sample_rate: data.sample_rate,
                        duration_ms: data.duration_ms,
                    },
                    has_more: *has_more,
                },
            };
            let header_json = serde_json::to_vec(&hdr)?;
            assemble_frame(&header_json, Some(&data.audio), flags, limits)
        }
        ProcessingResult::Failed { .. } => {
            let header_json = serde_json::to_vec(full_response)?;
            assemble_frame(&header_json, None, flags, limits)
        }
    }
}

fn encode_s2s_response(
    request_id: u64,
    result: &ProcessingResult<S2sOutput>,
    full_response: &EngineResponse,
    flags: FrameFlags,
    limits: FrameLimits,
) -> Result<Vec<u8>, FramingError> {
    match result {
        ProcessingResult::Success {
            data,
            processing_ms,
        } => {
            let hdr = BorrowedAudioResponseHeader::S2sResult {
                request_id,
                result: BorrowedProcessingResult::Success {
                    data: BorrowedS2sData {
                        audio: &[],
                        sample_rate: data.sample_rate,
                        transcription: &data.transcription,
                        translation: &data.translation,
                    },
                    processing_ms: *processing_ms,
                },
            };
            let header_json = serde_json::to_vec(&hdr)?;
            assemble_frame(&header_json, Some(&data.audio), flags, limits)
        }
        ProcessingResult::Partial { data, has_more } => {
            let hdr = BorrowedAudioResponseHeader::S2sResult {
                request_id,
                result: BorrowedProcessingResult::Partial {
                    data: BorrowedS2sData {
                        audio: &[],
                        sample_rate: data.sample_rate,
                        transcription: &data.transcription,
                        translation: &data.translation,
                    },
                    has_more: *has_more,
                },
            };
            let header_json = serde_json::to_vec(&hdr)?;
            assemble_frame(&header_json, Some(&data.audio), flags, limits)
        }
        ProcessingResult::Failed { .. } => {
            let header_json = serde_json::to_vec(full_response)?;
            assemble_frame(&header_json, None, flags, limits)
        }
    }
}

/// Rehydrate an [`EngineResponse`] from its validated prefix, JSON header, and PCM payload bytes.
pub fn rehydrate_response_from_parts(
    prefix: FramePrefix,
    header_bytes: &[u8],
    payload_bytes: &[u8],
) -> Result<(EngineResponse, FrameFlags), FramingError> {
    let mut response: EngineResponse = serde_json::from_slice(header_bytes)?;
    if prefix.flags.contains(FrameFlags::HAS_PCM_PAYLOAD) || !payload_bytes.is_empty() {
        let pcm = decode_pcm_bytes(payload_bytes)?;
        match &mut response {
            EngineResponse::TtsResult { result, .. } => match result {
                ProcessingResult::Success { data, .. } | ProcessingResult::Partial { data, .. } => {
                    data.audio = pcm;
                }
                ProcessingResult::Failed { .. } => {}
            },
            EngineResponse::S2sResult { result, .. } => match result {
                ProcessingResult::Success { data, .. } | ProcessingResult::Partial { data, .. } => {
                    data.audio = pcm;
                }
                ProcessingResult::Failed { .. } => {}
            },
            _ => {}
        }
    }
    Ok((response, prefix.flags))
}

/// Decode a complete byte slice containing one `OSBP` v1 response frame.
pub fn decode_response_frame(
    frame_bytes: &[u8],
    limits: FrameLimits,
) -> Result<(EngineResponse, FrameFlags), FramingError> {
    if frame_bytes.len() < FRAME_PREFIX_LEN {
        return Err(FramingError::UnexpectedEof);
    }
    let mut prefix_arr = [0u8; FRAME_PREFIX_LEN];
    prefix_arr.copy_from_slice(&frame_bytes[..FRAME_PREFIX_LEN]);
    let prefix = parse_frame_prefix(&prefix_arr, limits)?;

    let body_end = FRAME_PREFIX_LEN + prefix.header_len as usize + prefix.payload_len as usize;
    if frame_bytes.len() < body_end {
        return Err(FramingError::UnexpectedEof);
    }

    let header_end = FRAME_PREFIX_LEN + prefix.header_len as usize;
    rehydrate_response_from_parts(
        prefix,
        &frame_bytes[FRAME_PREFIX_LEN..header_end],
        &frame_bytes[header_end..body_end],
    )
}

fn read_exact_or_eof<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<(), FramingError> {
    let mut read_total = 0usize;
    while read_total < buf.len() {
        match reader.read(&mut buf[read_total..]) {
            Ok(0) => return Err(FramingError::UnexpectedEof),
            Ok(n) => read_total += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(FramingError::Io(e)),
        }
    }
    Ok(())
}

/// Synchronously read and decode a single request frame from `reader`.
pub fn read_request_frame<R: Read>(
    reader: &mut R,
    limits: FrameLimits,
) -> Result<(EngineRequest, FrameFlags), FramingError> {
    let mut prefix_arr = [0u8; FRAME_PREFIX_LEN];
    read_exact_or_eof(reader, &mut prefix_arr)?;
    let prefix = parse_frame_prefix(&prefix_arr, limits)?;

    let mut header_bytes = vec![0u8; prefix.header_len as usize];
    read_exact_or_eof(reader, &mut header_bytes)?;

    let mut payload_bytes = vec![0u8; prefix.payload_len as usize];
    if !payload_bytes.is_empty() {
        read_exact_or_eof(reader, &mut payload_bytes)?;
    }

    rehydrate_request_from_parts(prefix, &header_bytes, &payload_bytes)
}

/// Synchronously encode and write a single request frame to `writer`.
pub fn write_request_frame<W: Write>(
    writer: &mut W,
    request: &EngineRequest,
    flags: FrameFlags,
    limits: FrameLimits,
) -> Result<(), FramingError> {
    let bytes = encode_request_frame(request, flags, limits)?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

/// Synchronously read and decode a single response frame from `reader`.
pub fn read_response_frame<R: Read>(
    reader: &mut R,
    limits: FrameLimits,
) -> Result<(EngineResponse, FrameFlags), FramingError> {
    let mut prefix_arr = [0u8; FRAME_PREFIX_LEN];
    read_exact_or_eof(reader, &mut prefix_arr)?;
    let prefix = parse_frame_prefix(&prefix_arr, limits)?;

    let mut header_bytes = vec![0u8; prefix.header_len as usize];
    read_exact_or_eof(reader, &mut header_bytes)?;

    let mut payload_bytes = vec![0u8; prefix.payload_len as usize];
    if !payload_bytes.is_empty() {
        read_exact_or_eof(reader, &mut payload_bytes)?;
    }

    rehydrate_response_from_parts(prefix, &header_bytes, &payload_bytes)
}

/// Synchronously encode and write a single response frame to `writer`.
pub fn write_response_frame<W: Write>(
    writer: &mut W,
    response: &EngineResponse,
    flags: FrameFlags,
    limits: FrameLimits,
) -> Result<(), FramingError> {
    let bytes = encode_response_frame(response, flags, limits)?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

/// Asynchronous Tokio stream framing helpers (enabled with feature `async-io`).
#[cfg(feature = "async-io")]
pub mod tokio_io {
    use super::{
        encode_request_frame, encode_response_frame, parse_frame_prefix,
        rehydrate_request_from_parts, rehydrate_response_from_parts, FrameFlags, FrameLimits,
        FramePrefix, FramingError, FRAME_PREFIX_LEN,
    };
    use crate::message::{EngineRequest, EngineResponse};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

    async fn read_raw_parts_async<R: AsyncRead + Unpin>(
        reader: &mut R,
        limits: FrameLimits,
    ) -> Result<(FramePrefix, Vec<u8>, Vec<u8>), FramingError> {
        let mut prefix_arr = [0u8; FRAME_PREFIX_LEN];
        if let Err(err) = reader.read_exact(&mut prefix_arr).await {
            if err.kind() == std::io::ErrorKind::UnexpectedEof {
                return Err(FramingError::UnexpectedEof);
            }
            return Err(FramingError::Io(err));
        }

        let prefix = parse_frame_prefix(&prefix_arr, limits)?;

        let mut header_bytes = vec![0u8; prefix.header_len as usize];
        reader.read_exact(&mut header_bytes).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::UnexpectedEof {
                FramingError::UnexpectedEof
            } else {
                FramingError::Io(err)
            }
        })?;

        let mut payload_bytes = vec![0u8; prefix.payload_len as usize];
        if !payload_bytes.is_empty() {
            reader.read_exact(&mut payload_bytes).await.map_err(|err| {
                if err.kind() == std::io::ErrorKind::UnexpectedEof {
                    FramingError::UnexpectedEof
                } else {
                    FramingError::Io(err)
                }
            })?;
        }

        Ok((prefix, header_bytes, payload_bytes))
    }

    /// Asynchronously read and decode one request frame from `reader`.
    pub async fn read_request_frame_async<R: AsyncRead + Unpin>(
        reader: &mut R,
        limits: FrameLimits,
    ) -> Result<(EngineRequest, FrameFlags), FramingError> {
        let (prefix, header_bytes, payload_bytes) = read_raw_parts_async(reader, limits).await?;
        rehydrate_request_from_parts(prefix, &header_bytes, &payload_bytes)
    }

    /// Asynchronously encode and write one request frame to `writer`.
    pub async fn write_request_frame_async<W: AsyncWrite + Unpin>(
        writer: &mut W,
        request: &EngineRequest,
        flags: FrameFlags,
        limits: FrameLimits,
    ) -> Result<(), FramingError> {
        let bytes = encode_request_frame(request, flags, limits)?;
        writer.write_all(&bytes).await?;
        writer.flush().await?;
        Ok(())
    }

    /// Asynchronously read and decode one response frame from `reader`.
    pub async fn read_response_frame_async<R: AsyncRead + Unpin>(
        reader: &mut R,
        limits: FrameLimits,
    ) -> Result<(EngineResponse, FrameFlags), FramingError> {
        let (prefix, header_bytes, payload_bytes) = read_raw_parts_async(reader, limits).await?;
        rehydrate_response_from_parts(prefix, &header_bytes, &payload_bytes)
    }

    /// Asynchronously encode and write one response frame to `writer`.
    pub async fn write_response_frame_async<W: AsyncWrite + Unpin>(
        writer: &mut W,
        response: &EngineResponse,
        flags: FrameFlags,
        limits: FrameLimits,
    ) -> Result<(), FramingError> {
        let bytes = encode_response_frame(response, flags, limits)?;
        writer.write_all(&bytes).await?;
        writer.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PROTOCOL_VERSION;
    use proptest::prelude::*;
    use std::io::Cursor;

    #[test]
    fn test_binary_pcm_stripped_from_json_and_restored_bit_exact() {
        let samples = vec![-1.0f32, -0.25, 0.0, std::f32::consts::PI / 4.0, 0.999_999];
        let req = EngineRequest::ProcessSpeechTranslation {
            request_id: 42,
            audio: samples.clone(),
            sample_rate: 16_000,
            source_lang: "pt-BR".to_string(),
            target_lang: "en-US".to_string(),
            is_final: true,
            meta: Some(AudioFrameMeta {
                session_id: 1,
                utterance_id: 7,
                chunk_id: 3,
                timestamp_us: 123_456,
                channels: 1,
            }),
        };

        let encoded =
            encode_request_frame(&req, FrameFlags::SPEECH_END, FrameLimits::default()).unwrap();

        let mut prefix = [0u8; FRAME_PREFIX_LEN];
        prefix.copy_from_slice(&encoded[..FRAME_PREFIX_LEN]);
        let parsed_prefix = parse_frame_prefix(&prefix, FrameLimits::default()).unwrap();

        assert!(parsed_prefix.flags.contains(FrameFlags::HAS_PCM_PAYLOAD));
        assert!(parsed_prefix.flags.contains(FrameFlags::SPEECH_END));
        assert!(parsed_prefix.flags.contains(FrameFlags::END_OF_UTTERANCE));
        assert_eq!(parsed_prefix.payload_len as usize, samples.len() * 4);

        let header_str = std::str::from_utf8(
            &encoded[FRAME_PREFIX_LEN..FRAME_PREFIX_LEN + parsed_prefix.header_len as usize],
        )
        .unwrap();
        assert!(header_str.contains(r#""audio":[]"#));

        let (decoded, flags) = decode_request_frame(&encoded, FrameLimits::default()).unwrap();
        assert!(flags.contains(FrameFlags::HAS_PCM_PAYLOAD));
        match decoded {
            EngineRequest::ProcessSpeechTranslation {
                request_id,
                audio,
                sample_rate,
                meta,
                ..
            } => {
                assert_eq!(request_id, 42);
                assert_eq!(sample_rate, 16_000);
                assert_eq!(audio, samples);
                assert_eq!(meta.unwrap().utterance_id, 7);
            }
            other => panic!("unexpected decoded request: {other:?}"),
        }
    }

    #[test]
    fn test_tts_response_binary_roundtrip_over_stream() {
        let samples = vec![0.1f32, -0.2, 0.3, -0.4, 0.5];
        let resp = EngineResponse::TtsResult {
            request_id: 9,
            result: ProcessingResult::Success {
                data: TtsOutput {
                    audio: samples.clone(),
                    sample_rate: 24_000,
                    duration_ms: 250,
                },
                processing_ms: 18,
            },
        };

        let mut wire = Vec::new();
        write_response_frame(&mut wire, &resp, FrameFlags::NONE, FrameLimits::default()).unwrap();

        let mut cursor = Cursor::new(wire);
        let (decoded, flags) = read_response_frame(&mut cursor, FrameLimits::default()).unwrap();
        assert!(flags.contains(FrameFlags::HAS_PCM_PAYLOAD));

        match decoded {
            EngineResponse::TtsResult { request_id, result } => {
                assert_eq!(request_id, 9);
                let data = result.data().unwrap();
                assert_eq!(data.audio, samples);
                assert_eq!(data.sample_rate, 24_000);
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn test_rejects_oversized_and_malformed_prefixes_without_allocating() {
        let limits = FrameLimits {
            max_header_bytes: 1024,
            max_payload_bytes: 4096,
        };

        let mut bad_magic = [0u8; FRAME_PREFIX_LEN];
        bad_magic[..4].copy_from_slice(b"NOPE");
        assert!(matches!(
            parse_frame_prefix(&bad_magic, limits),
            Err(FramingError::InvalidMagic(_))
        ));

        let mut bad_version = [0u8; FRAME_PREFIX_LEN];
        bad_version[..4].copy_from_slice(&WIRE_MAGIC);
        bad_version[4..6].copy_from_slice(&99u16.to_le_bytes());
        assert!(matches!(
            parse_frame_prefix(&bad_version, limits),
            Err(FramingError::UnsupportedVersion(99))
        ));

        let mut huge_payload = [0u8; FRAME_PREFIX_LEN];
        huge_payload[..4].copy_from_slice(&WIRE_MAGIC);
        huge_payload[4..6].copy_from_slice(&WIRE_VERSION.to_le_bytes());
        huge_payload[8..12].copy_from_slice(&64u32.to_le_bytes());
        huge_payload[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            parse_frame_prefix(&huge_payload, limits),
            Err(FramingError::PayloadTooLarge { .. })
        ));

        let mut unaligned = [0u8; FRAME_PREFIX_LEN];
        unaligned[..4].copy_from_slice(&WIRE_MAGIC);
        unaligned[4..6].copy_from_slice(&WIRE_VERSION.to_le_bytes());
        unaligned[6..8].copy_from_slice(&FrameFlags::HAS_PCM_PAYLOAD.bits().to_le_bytes());
        unaligned[8..12].copy_from_slice(&32u32.to_le_bytes());
        unaligned[12..16].copy_from_slice(&7u32.to_le_bytes());
        assert!(matches!(
            parse_frame_prefix(&unaligned, limits),
            Err(FramingError::UnalignedPcmPayload { byte_len: 7 })
        ));
    }

    #[test]
    fn test_handshake_hello_frame_roundtrip() {
        let hello = EngineRequest::Hello {
            protocol_version: PROTOCOL_VERSION.to_string(),
            max_in_flight: 4,
            max_payload_bytes: DEFAULT_MAX_PAYLOAD_BYTES,
        };
        let bytes = encode_request_frame(&hello, FrameFlags::NONE, FrameLimits::default()).unwrap();
        let (decoded, flags) = decode_request_frame(&bytes, FrameLimits::default()).unwrap();
        assert_eq!(flags, FrameFlags::NONE);
        match decoded {
            EngineRequest::Hello {
                protocol_version,
                max_in_flight,
                ..
            } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION);
                assert_eq!(max_in_flight, 4);
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    proptest! {
        #[test]
        fn prop_pcm_stt_frame_roundtrip(
            req_id in any::<u64>(),
            samples in proptest::collection::vec(-1.0f32..1.0f32, 0..512),
            is_final in any::<bool>(),
        ) {
            let req = EngineRequest::ProcessStt {
                request_id: req_id,
                audio: samples.clone(),
                sample_rate: 16_000,
                is_final,
            };
            let bytes = encode_request_frame(&req, FrameFlags::NONE, FrameLimits::default()).unwrap();
            let (decoded, _) = decode_request_frame(&bytes, FrameLimits::default()).unwrap();
            match decoded {
                EngineRequest::ProcessStt { request_id, audio, is_final: fin, .. } => {
                    prop_assert_eq!(request_id, req_id);
                    prop_assert_eq!(fin, is_final);
                    prop_assert_eq!(audio, samples);
                }
                _ => prop_assert!(false, "decoded unexpected variant"),
            }
        }
    }
}
