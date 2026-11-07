"""OSBP v1 binary wire framing codec for Unix Domain Socket engine workers.

Wire layout (little-endian):
    0..4   magic: b"OSBP"
    4..6   version: u16 (1)
    6..8   flags: u16
    8..12  header_len: u32 (UTF-8 JSON control header bytes)
    12..16 payload_len: u32 (raw f32le PCM bytes, multiple of 4)
"""

from __future__ import annotations

import array
import json
import socket
import struct
import sys
from dataclasses import dataclass
from typing import Any, Iterable

WIRE_MAGIC = b"OSBP"
WIRE_VERSION = 1
FRAME_PREFIX_LEN = 16
DEFAULT_MAX_HEADER_BYTES = 64 * 1024
DEFAULT_MAX_PAYLOAD_BYTES = 4 * 1024 * 1024

_PREFIX_STRUCT = struct.Struct("<4sHHII")


class FrameFlags:
    """Bitflags carried in the 16-byte OSBP v1 prefix."""

    NONE = 0
    HAS_PCM_PAYLOAD = 1 << 0
    SPEECH_START = 1 << 1
    SPEECH_END = 1 << 2
    END_OF_UTTERANCE = 1 << 3
    END_OF_STREAM = 1 << 4
    DISCONTINUITY = 1 << 5


@dataclass(frozen=True)
class FrameLimits:
    """Upper bounds validated prior to allocating frame buffers."""

    max_header_bytes: int = DEFAULT_MAX_HEADER_BYTES
    max_payload_bytes: int = DEFAULT_MAX_PAYLOAD_BYTES


@dataclass(frozen=True)
class DecodedFrame:
    """Decoded OSBP v1 frame with parsed JSON control header and raw f32le PCM bytes."""

    flags: int
    header: dict[str, Any]
    pcm_bytes: bytes


class FramingError(Exception):
    """Raised on malformed, oversized, or truncated OSBP v1 frames."""


class UnexpectedEofError(FramingError):
    """Raised when the peer closes the UDS stream cleanly at a frame boundary."""


def is_version_compatible(local: str, remote: str) -> bool:
    """Check SemVer compatibility matching osb-protocol rules."""
    try:
        l_parts = [int(p) for p in local.strip().split(".")[:2]]
        r_parts = [int(p) for p in remote.strip().split(".")[:2]]
    except ValueError:
        return False
    if len(l_parts) < 2 or len(r_parts) < 2:
        return False
    if l_parts[0] == 0 or r_parts[0] == 0:
        return l_parts[0] == r_parts[0] and l_parts[1] == r_parts[1]
    return l_parts[0] == r_parts[0]


def pcm_floats_to_bytes(samples: Iterable[float] | bytes | bytearray | memoryview) -> bytes:
    """Pack float32 samples into contiguous little-endian f32le bytes."""
    if isinstance(samples, (bytes, bytearray, memoryview)):
        raw = bytes(samples)
        if len(raw) % 4 != 0:
            raise FramingError(f"unaligned f32le PCM byte buffer: {len(raw)} bytes")
        return raw

    tobytes_fn = getattr(samples, "astype", None)
    if callable(tobytes_fn):
        return samples.astype("<f4", copy=False).tobytes()  # type: ignore[no-any-return]

    arr = array.array("f", samples)
    if sys.byteorder != "little":
        arr.byteswap()
    return arr.tobytes()


def pcm_bytes_to_floats(pcm_bytes: bytes) -> list[float]:
    """Unpack little-endian f32le bytes into Python floats."""
    if not pcm_bytes:
        return []
    if len(pcm_bytes) % 4 != 0:
        raise FramingError(f"unaligned f32le PCM payload: {len(pcm_bytes)} bytes")
    arr = array.array("f")
    arr.frombytes(pcm_bytes)
    if sys.byteorder != "little":
        arr.byteswap()
    return arr.tolist()


def parse_frame_prefix(
    prefix: bytes, limits: FrameLimits = FrameLimits()
) -> tuple[int, int, int]:
    """Validate a 16-byte wire prefix before allocating body buffers."""
    if len(prefix) != FRAME_PREFIX_LEN:
        raise FramingError(f"expected {FRAME_PREFIX_LEN}-byte prefix, got {len(prefix)}")
    magic, version, flags, header_len, payload_len = _PREFIX_STRUCT.unpack(prefix)
    if magic != WIRE_MAGIC:
        raise FramingError(f"invalid frame magic: {magic!r}")
    if version != WIRE_VERSION:
        raise FramingError(f"unsupported wire frame version: {version}")
    if header_len > limits.max_header_bytes:
        raise FramingError(
            f"control header size {header_len} exceeds limit {limits.max_header_bytes}"
        )
    if payload_len > limits.max_payload_bytes:
        raise FramingError(
            f"PCM payload size {payload_len} exceeds limit {limits.max_payload_bytes}"
        )
    if ((flags & FrameFlags.HAS_PCM_PAYLOAD) or payload_len > 0) and (payload_len % 4 != 0):
        raise FramingError(f"unaligned f32le PCM payload size: {payload_len} bytes")
    return flags, header_len, payload_len


def _extract_embedded_pcm(header: dict[str, Any]) -> tuple[dict[str, Any], bytes]:
    """Strip audio arrays out of JSON control messages into binary f32le bytes."""
    msg_type = header.get("type")
    if msg_type in ("process_stt", "process_speech_translation", "process_s2s"):
        audio = header.get("audio")
        if audio:
            copied = dict(header)
            copied["audio"] = []
            return copied, pcm_floats_to_bytes(audio)
    elif msg_type in ("tts_result", "s2s_result"):
        result = header.get("result")
        if isinstance(result, dict) and result.get("status") in ("success", "partial"):
            data = result.get("data")
            if isinstance(data, dict) and data.get("audio"):
                raw_pcm = pcm_floats_to_bytes(data["audio"])
                data_copy = dict(data)
                data_copy["audio"] = []
                res_copy = dict(result)
                res_copy["data"] = data_copy
                hdr_copy = dict(header)
                hdr_copy["result"] = res_copy
                return hdr_copy, raw_pcm
    return header, b""


def encode_frame_bytes(
    header: dict[str, Any],
    pcm_bytes: bytes = b"",
    flags: int = FrameFlags.NONE,
    limits: FrameLimits = FrameLimits(),
) -> bytes:
    """Encode a control message dictionary and optional f32le PCM bytes into an OSBP v1 frame."""
    if not pcm_bytes:
        header, pcm_bytes = _extract_embedded_pcm(header)

    if len(pcm_bytes) % 4 != 0:
        raise FramingError(f"unaligned f32le PCM payload size: {len(pcm_bytes)} bytes")

    if pcm_bytes:
        flags |= FrameFlags.HAS_PCM_PAYLOAD
    else:
        flags &= ~FrameFlags.HAS_PCM_PAYLOAD

    header_json = json.dumps(header, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    header_len = len(header_json)
    payload_len = len(pcm_bytes)

    if header_len > limits.max_header_bytes:
        raise FramingError(
            f"control header size {header_len} exceeds limit {limits.max_header_bytes}"
        )
    if payload_len > limits.max_payload_bytes:
        raise FramingError(
            f"PCM payload size {payload_len} exceeds limit {limits.max_payload_bytes}"
        )

    prefix = _PREFIX_STRUCT.pack(WIRE_MAGIC, WIRE_VERSION, flags, header_len, payload_len)
    return prefix + header_json + pcm_bytes


def decode_frame_bytes(
    frame_bytes: bytes, limits: FrameLimits = FrameLimits()
) -> DecodedFrame:
    """Decode a complete OSBP v1 frame from bytes."""
    if len(frame_bytes) < FRAME_PREFIX_LEN:
        raise UnexpectedEofError("truncated frame prefix")
    flags, header_len, payload_len = parse_frame_prefix(
        frame_bytes[:FRAME_PREFIX_LEN], limits
    )
    expected_total = FRAME_PREFIX_LEN + header_len + payload_len
    if len(frame_bytes) < expected_total:
        raise UnexpectedEofError("truncated frame body")
    header_slice = frame_bytes[FRAME_PREFIX_LEN : FRAME_PREFIX_LEN + header_len]
    pcm_slice = frame_bytes[FRAME_PREFIX_LEN + header_len : expected_total]
    try:
        header = json.loads(header_slice.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise FramingError(f"invalid JSON control header: {exc}") from exc
    if not isinstance(header, dict):
        raise FramingError("JSON control header must be an object")
    return DecodedFrame(flags=flags, header=header, pcm_bytes=pcm_slice)


def _recv_exact(sock: socket.socket, size: int, allow_clean_eof: bool = False) -> bytes:
    """Read exact byte count from a stream socket."""
    if size == 0:
        return b""
    buf = bytearray(size)
    view = memoryview(buf)
    offset = 0
    while offset < size:
        chunk_len = sock.recv_into(view[offset:], size - offset)
        if chunk_len == 0:
            if allow_clean_eof and offset == 0:
                raise UnexpectedEofError("clean EOF at frame boundary")
            raise FramingError("unexpected EOF mid-frame")
        offset += chunk_len
    return bytes(buf)


def read_frame(sock: socket.socket, limits: FrameLimits = FrameLimits()) -> DecodedFrame:
    """Read and validate one OSBP v1 frame from a connected socket."""
    prefix = _recv_exact(sock, FRAME_PREFIX_LEN, allow_clean_eof=True)
    flags, header_len, payload_len = parse_frame_prefix(prefix, limits)
    header_raw = _recv_exact(sock, header_len)
    pcm_bytes = _recv_exact(sock, payload_len)
    try:
        header = json.loads(header_raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise FramingError(f"invalid JSON control header: {exc}") from exc
    if not isinstance(header, dict):
        raise FramingError("JSON control header must be an object")
    return DecodedFrame(flags=flags, header=header, pcm_bytes=pcm_bytes)


def write_frame(
    sock: socket.socket,
    header: dict[str, Any],
    pcm_bytes: bytes = b"",
    flags: int = FrameFlags.NONE,
    limits: FrameLimits = FrameLimits(),
) -> None:
    """Encode and write one OSBP v1 frame atomically to a connected socket."""
    encoded = encode_frame_bytes(header, pcm_bytes=pcm_bytes, flags=flags, limits=limits)
    sock.sendall(encoded)
