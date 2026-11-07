"""Persistent Unix Domain Socket worker runtime for OpenSpeechBridge engines.

Privacy invariant:
    Never log raw audio samples, user transcripts, or translated text.
    Log only lifecycle events, engine IDs, request IDs, durations, and error codes.
"""

from __future__ import annotations

import argparse
import os
import socket
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from typing import Any

from .framing import (
    DEFAULT_MAX_PAYLOAD_BYTES,
    DecodedFrame,
    FrameFlags,
    FrameLimits,
    FramingError,
    UnexpectedEofError,
    is_version_compatible,
    read_frame,
    write_frame,
)

PROTOCOL_VERSION = "0.1.0"
DEFAULT_MAX_IN_FLIGHT = 4


def _log(msg: str) -> None:
    sys.stderr.write(f"[osb-py-worker] {msg}\n")
    sys.stderr.flush()


class BaseEngineWorker:
    """Base class for persistent local ML engine workers over Unix Domain Sockets."""

    def __init__(
        self,
        max_in_flight: int = DEFAULT_MAX_IN_FLIGHT,
        limits: FrameLimits = FrameLimits(),
    ) -> None:
        self.max_in_flight = max(1, max_in_flight)
        self.limits = limits
        self._write_lock = threading.Lock()
        self._cancel_lock = threading.Lock()
        self._cancelled_ids: set[int] = set()

    def engine_info(self) -> dict[str, Any]:
        """Return OSBP EngineInfo metadata dictionary."""
        raise NotImplementedError

    def on_initialize(self, config: Any) -> None:
        """Load models and warm up resources during handshake."""

    def on_shutdown(self) -> None:
        """Release resources before process exit."""

    def process_stt(
        self, request: dict[str, Any], pcm_bytes: bytes, flags: int
    ) -> dict[str, Any]:
        raise NotImplementedError("process_stt not supported by this engine")

    def process_speech_translation(
        self, request: dict[str, Any], pcm_bytes: bytes, flags: int
    ) -> dict[str, Any]:
        raise NotImplementedError(
            "process_speech_translation not supported by this engine"
        )

    def process_translation(self, request: dict[str, Any]) -> dict[str, Any]:
        raise NotImplementedError("process_translation not supported by this engine")

    def process_tts(self, request: dict[str, Any]) -> tuple[dict[str, Any], bytes]:
        raise NotImplementedError("process_tts not supported by this engine")

    def process_s2s(
        self, request: dict[str, Any], pcm_bytes: bytes, flags: int
    ) -> tuple[dict[str, Any], bytes]:
        raise NotImplementedError("process_s2s not supported by this engine")

    def is_cancelled(self, request_id: int) -> bool:
        with self._cancel_lock:
            return request_id in self._cancelled_ids

    def _mark_cancelled(self, request_id: int) -> None:
        with self._cancel_lock:
            if len(self._cancelled_ids) >= 1024:
                self._cancelled_ids.clear()
            self._cancelled_ids.add(request_id)

    def _clear_cancelled(self, request_id: int) -> None:
        with self._cancel_lock:
            self._cancelled_ids.discard(request_id)

    def _send(
        self,
        sock: socket.socket,
        header: dict[str, Any],
        pcm_bytes: bytes = b"",
        flags: int = FrameFlags.NONE,
    ) -> None:
        with self._write_lock:
            write_frame(
                sock, header, pcm_bytes=pcm_bytes, flags=flags, limits=self.limits
            )

    def _handle_hello(self, req: dict[str, Any]) -> dict[str, Any]:
        remote_ver = str(req.get("protocol_version", ""))
        if not is_version_compatible(PROTOCOL_VERSION, remote_ver):
            return {
                "type": "error",
                "request_id": None,
                "code": "INCOMPATIBLE_PROTOCOL",
                "message": f"worker protocol {PROTOCOL_VERSION} incompatible with {remote_ver}",
            }
        peer_flight = max(1, int(req.get("max_in_flight", self.max_in_flight)))
        peer_payload = int(req.get("max_payload_bytes", DEFAULT_MAX_PAYLOAD_BYTES))
        negotiated_payload = min(self.limits.max_payload_bytes, peer_payload)
        self.limits = FrameLimits(
            max_header_bytes=self.limits.max_header_bytes,
            max_payload_bytes=negotiated_payload,
        )
        return {
            "type": "welcome",
            "protocol_version": PROTOCOL_VERSION,
            "info": self.engine_info(),
            "max_in_flight": min(self.max_in_flight, peer_flight),
            "max_payload_bytes": negotiated_payload,
        }

    def _execute_inference_frame(
        self, sock: socket.socket, frame: DecodedFrame
    ) -> None:
        req = frame.header
        msg_type = str(req.get("type", ""))
        req_id = int(req.get("request_id", 0))

        if self.is_cancelled(req_id):
            self._clear_cancelled(req_id)
            return

        try:
            resp_hdr, pcm_out = self._dispatch_inference(msg_type, req, frame)
            if self.is_cancelled(req_id):
                self._clear_cancelled(req_id)
                return
            out_flags = FrameFlags.END_OF_UTTERANCE if pcm_out else FrameFlags.NONE
            self._send(sock, resp_hdr, pcm_bytes=pcm_out, flags=out_flags)
        except NotImplementedError as exc:
            self._send(
                sock,
                {
                    "type": "error",
                    "request_id": req_id,
                    "code": "UNSUPPORTED_CAPABILITY",
                    "message": str(exc),
                },
            )
        except Exception as exc:  # noqa: BLE001
            _log(f"request_id={req_id} inference failed: {exc.__class__.__name__}")
            self._send(
                sock,
                {
                    "type": "error",
                    "request_id": req_id,
                    "code": "INFERENCE_FAILED",
                    "message": str(exc),
                },
            )
        finally:
            self._clear_cancelled(req_id)

    def _dispatch_inference(
        self, msg_type: str, req: dict[str, Any], frame: DecodedFrame
    ) -> tuple[dict[str, Any], bytes]:
        if msg_type == "process_stt":
            return self.process_stt(req, frame.pcm_bytes, frame.flags), b""
        if msg_type == "process_speech_translation":
            return (
                self.process_speech_translation(req, frame.pcm_bytes, frame.flags),
                b"",
            )
        if msg_type == "process_translation":
            return self.process_translation(req), b""
        if msg_type == "process_tts":
            return self.process_tts(req)
        if msg_type == "process_s2s":
            return self.process_s2s(req, frame.pcm_bytes, frame.flags)
        raise NotImplementedError(f"unknown processing request type: {msg_type}")

    def serve_connection(self, sock: socket.socket) -> None:
        """Serve OSBP v1 requests over an established Unix Domain Socket."""
        executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="osb-ml")
        try:
            while True:
                frame = read_frame(sock, limits=self.limits)
                msg_type = str(frame.header.get("type", ""))

                if msg_type == "hello":
                    self._send(sock, self._handle_hello(frame.header))
                elif msg_type == "initialize":
                    self._handle_init_message(sock, frame.header)
                elif msg_type == "get_capabilities":
                    self._send(sock, {"type": "capabilities", "info": self.engine_info()})
                elif msg_type == "ping":
                    now_ms = int(time.time() * 1000)
                    self._send(
                        sock,
                        {
                            "type": "pong",
                            "timestamp_ms": int(frame.header.get("timestamp_ms", 0)),
                            "engine_timestamp_ms": now_ms,
                        },
                    )
                elif msg_type == "cancel":
                    cid = int(frame.header.get("request_id", 0))
                    self._mark_cancelled(cid)
                    self._send(sock, {"type": "cancelled", "request_id": cid})
                elif msg_type == "shutdown":
                    executor.shutdown(wait=True)
                    self.on_shutdown()
                    self._send(sock, {"type": "shutdown_complete"})
                    return
                else:
                    executor.submit(self._execute_inference_frame, sock, frame)
        except UnexpectedEofError:
            pass
        except (FramingError, OSError) as exc:
            _log(f"transport closed: {exc}")
        finally:
            executor.shutdown(wait=False, cancel_futures=True)

    def _handle_init_message(self, sock: socket.socket, req: dict[str, Any]) -> None:
        try:
            self.on_initialize(req.get("config"))
            engine_id = str(self.engine_info().get("id", "unknown"))
            self._send(sock, {"type": "initialized", "engine_id": engine_id})
        except Exception as exc:  # noqa: BLE001
            _log(f"initialization failed: {exc}")
            self._send(
                sock,
                {
                    "type": "error",
                    "request_id": None,
                    "code": "INIT_FAILED",
                    "message": str(exc),
                },
            )

    def connect_and_serve(self, socket_path: str) -> None:
        """Connect to a daemon-bound UDS socket and serve until shutdown or EOF."""
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.connect(socket_path)
            self.serve_connection(sock)

    def listen_and_serve(self, socket_path: str) -> None:
        """Bind a standalone UDS listener (attach mode) and serve connections sequentially."""
        if os.path.exists(socket_path):
            os.unlink(socket_path)
        parent = os.path.dirname(os.path.abspath(socket_path))
        os.makedirs(parent, exist_ok=True)
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
            listener.bind(socket_path)
            os.chmod(socket_path, 0o600)
            listener.listen(1)
            try:
                conn, _ = listener.accept()
                with conn:
                    self.serve_connection(conn)
            finally:
                if os.path.exists(socket_path):
                    os.unlink(socket_path)

    @staticmethod
    def add_common_cli_args(parser: argparse.ArgumentParser) -> None:
        """Register standard `--socket` and `--listen` arguments on a worker CLI parser."""
        parser.add_argument(
            "--socket",
            default=os.environ.get("OSB_ENGINE_SOCKET"),
            help="Connect to daemon-bound Unix Domain Socket path (or OSB_ENGINE_SOCKET)",
        )
        parser.add_argument(
            "--listen",
            default=None,
            help="Bind and listen on Unix Domain Socket path (standalone attach mode)",
        )

    def run_from_parsed_args(self, args: argparse.Namespace) -> None:
        """Start worker transport based on parsed CLI arguments."""
        if args.listen:
            self.listen_and_serve(args.listen)
        elif args.socket:
            self.connect_and_serve(args.socket)
        else:
            raise SystemExit(
                "missing UDS path: pass --socket <path>, --listen <path>, or set OSB_ENGINE_SOCKET"
            )
