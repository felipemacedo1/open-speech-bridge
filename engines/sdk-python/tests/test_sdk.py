"""Unit and UDS integration tests for the OpenSpeechBridge Python Engine SDK."""

from __future__ import annotations

import os
import socket
import struct
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from typing import Any

SDK_ROOT = Path(__file__).resolve().parents[1]
if str(SDK_ROOT) not in sys.path:
    sys.path.insert(0, str(SDK_ROOT))

from osb_engine_sdk import (  # noqa: E402
    PROTOCOL_VERSION,
    WIRE_MAGIC,
    WIRE_VERSION,
    BaseEngineWorker,
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


class DummyWorker(BaseEngineWorker):
    def engine_info(self) -> dict[str, Any]:
        return {
            "id": "py-dummy",
            "name": "Python Dummy Engine",
            "version": "0.1.0",
            "engine_type": "python",
            "capabilities": {
                "capabilities": ["speech_translation", "tts", "cpu_only"],
                "language_pairs": [{"source": "pt-BR", "target": "en-US"}],
                "stt_languages": [],
                "tts_languages": ["en-US"],
            },
            "model": None,
            "license": "Apache-2.0",
            "weights_license": None,
            "commercial_use": True,
        }

    def process_speech_translation(
        self, request: dict[str, Any], pcm_bytes: bytes, flags: int
    ) -> dict[str, Any]:
        samples = pcm_bytes_to_floats(pcm_bytes)
        has_eou = bool(flags & FrameFlags.END_OF_UTTERANCE)
        return {
            "type": "speech_translation_result",
            "request_id": int(request["request_id"]),
            "result": {
                "status": "success",
                "processing_ms": 2,
                "data": {
                    "text": f"samples={len(samples)};eou={has_eou}",
                    "detected_source": "pt",
                    "target_lang": request["target_lang"],
                    "confidence": 0.99,
                },
            },
        }

    def process_tts(self, request: dict[str, Any]) -> tuple[dict[str, Any], bytes]:
        pcm = pcm_floats_to_bytes([0.25, -0.25, 0.5, -0.5])
        return (
            {
                "type": "tts_result",
                "request_id": int(request["request_id"]),
                "result": {
                    "status": "success",
                    "processing_ms": 1,
                    "data": {
                        "audio": [],
                        "sample_rate": 24000,
                        "duration_ms": 1,
                    },
                },
            },
            pcm,
        )


class TestOsbpPythonSdk(unittest.TestCase):
    def test_binary_pcm_stripped_and_roundtripped_bit_exact(self) -> None:
        samples = [0.0, 0.125, -0.5, 0.875, -1.0]
        raw_frame = encode_frame_bytes(
            {
                "type": "process_speech_translation",
                "request_id": 42,
                "audio": samples,
                "sample_rate": 16000,
                "source_lang": "pt-BR",
                "target_lang": "en-US",
                "is_final": True,
            },
            flags=FrameFlags.END_OF_UTTERANCE,
        )
        decoded = decode_frame_bytes(raw_frame)
        self.assertTrue(decoded.flags & FrameFlags.HAS_PCM_PAYLOAD)
        self.assertTrue(decoded.flags & FrameFlags.END_OF_UTTERANCE)
        self.assertEqual(decoded.header["audio"], [])
        self.assertEqual(pcm_bytes_to_floats(decoded.pcm_bytes), samples)

    def test_rejects_malformed_and_oversized_prefixes(self) -> None:
        limits = FrameLimits(max_header_bytes=128, max_payload_bytes=256)

        bad_magic = struct.pack("<4sHHII", b"NOPE", WIRE_VERSION, 0, 2, 0) + b"{}"
        with self.assertRaises(FramingError):
            decode_frame_bytes(bad_magic, limits)

        bad_ver = struct.pack("<4sHHII", WIRE_MAGIC, 99, 0, 2, 0) + b"{}"
        with self.assertRaises(FramingError):
            decode_frame_bytes(bad_ver, limits)

        huge_hdr = struct.pack("<4sHHII", WIRE_MAGIC, WIRE_VERSION, 0, 9999, 0)
        with self.assertRaises(FramingError):
            decode_frame_bytes(huge_hdr, limits)

        unaligned = struct.pack(
            "<4sHHII", WIRE_MAGIC, WIRE_VERSION, FrameFlags.HAS_PCM_PAYLOAD, 2, 3
        )
        with self.assertRaises(FramingError):
            decode_frame_bytes(unaligned, limits)

    def test_semver_compatibility(self) -> None:
        self.assertTrue(is_version_compatible("0.1.0", "0.1.4"))
        self.assertFalse(is_version_compatible("0.1.0", "0.2.0"))
        self.assertTrue(is_version_compatible("1.0.0", "1.3.2"))
        self.assertFalse(is_version_compatible("1.0.0", "2.0.0"))

    def test_uds_worker_handshake_and_binary_inference_roundtrip(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            sock_path = os.path.join(tmpdir, "worker.sock")
            listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            listener.bind(sock_path)
            listener.listen(1)

            worker = DummyWorker()
            thread = threading.Thread(
                target=worker.connect_and_serve, args=(sock_path,), daemon=True
            )
            thread.start()

            conn, _ = listener.accept()
            with conn:
                write_frame(
                    conn,
                    {
                        "type": "hello",
                        "protocol_version": PROTOCOL_VERSION,
                        "max_in_flight": 4,
                        "max_payload_bytes": 1024 * 1024,
                    },
                )
                welcome = read_frame(conn)
                self.assertEqual(welcome.header["type"], "welcome")
                self.assertEqual(welcome.header["info"]["id"], "py-dummy")

                write_frame(conn, {"type": "initialize", "config": None})
                init_resp = read_frame(conn)
                self.assertEqual(init_resp.header["type"], "initialized")

                write_frame(conn, {"type": "ping", "timestamp_ms": 12345})
                pong = read_frame(conn)
                self.assertEqual(pong.header["type"], "pong")
                self.assertEqual(pong.header["timestamp_ms"], 12345)

                in_pcm = pcm_floats_to_bytes([0.1] * 160)
                write_frame(
                    conn,
                    {
                        "type": "process_speech_translation",
                        "request_id": 7,
                        "audio": [],
                        "sample_rate": 16000,
                        "source_lang": "pt-BR",
                        "target_lang": "en-US",
                        "is_final": True,
                    },
                    pcm_bytes=in_pcm,
                    flags=FrameFlags.END_OF_UTTERANCE,
                )
                st_resp = read_frame(conn)
                self.assertEqual(st_resp.header["type"], "speech_translation_result")
                self.assertEqual(st_resp.header["request_id"], 7)
                self.assertEqual(
                    st_resp.header["result"]["data"]["text"], "samples=160;eou=True"
                )

                write_frame(
                    conn,
                    {
                        "type": "process_tts",
                        "request_id": 8,
                        "text": "Hello world",
                        "language": "en-US",
                        "voice_id": None,
                    },
                )
                tts_resp = read_frame(conn)
                self.assertEqual(tts_resp.header["type"], "tts_result")
                self.assertEqual(
                    pcm_bytes_to_floats(tts_resp.pcm_bytes),
                    [0.25, -0.25, 0.5, -0.5],
                )

                write_frame(conn, {"type": "shutdown"})
                done = read_frame(conn)
                self.assertEqual(done.header["type"], "shutdown_complete")

            listener.close()
            thread.join(timeout=2.0)
            self.assertFalse(thread.is_alive())


if __name__ == "__main__":
    unittest.main()
