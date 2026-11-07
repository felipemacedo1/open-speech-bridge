"""OpenSpeechBridge Python Engine SDK (OSBP v1 over Unix Domain Sockets)."""

from .framing import (
    DEFAULT_MAX_HEADER_BYTES,
    DEFAULT_MAX_PAYLOAD_BYTES,
    FRAME_PREFIX_LEN,
    WIRE_MAGIC,
    WIRE_VERSION,
    DecodedFrame,
    FrameFlags,
    FrameLimits,
    FramingError,
    decode_frame_bytes,
    encode_frame_bytes,
    is_version_compatible,
    pcm_bytes_to_floats,
    pcm_floats_to_bytes,
    read_frame,
    write_frame,
)
from .worker import PROTOCOL_VERSION, BaseEngineWorker

__all__ = [
    "PROTOCOL_VERSION",
    "WIRE_MAGIC",
    "WIRE_VERSION",
    "FRAME_PREFIX_LEN",
    "DEFAULT_MAX_HEADER_BYTES",
    "DEFAULT_MAX_PAYLOAD_BYTES",
    "FrameFlags",
    "FrameLimits",
    "FramingError",
    "DecodedFrame",
    "BaseEngineWorker",
    "encode_frame_bytes",
    "decode_frame_bytes",
    "read_frame",
    "write_frame",
    "pcm_bytes_to_floats",
    "pcm_floats_to_bytes",
    "is_version_compatible",
]
