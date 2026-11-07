#!/usr/bin/env python3
"""Persistent CPU Kokoro-82M TTS UDS worker for OpenSpeechBridge (OSBP v1).

Synthesizes 24 kHz mono float32 PCM audio and returns it in the binary `f32le`
payload section without JSON float array overhead.
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from pathlib import Path
from typing import Any

# Allow direct script execution (`python3 .../kokoro_worker.py`) without pip install.
_SDK_ROOT = Path(__file__).resolve().parents[2]
if str(_SDK_ROOT) not in sys.path:
    sys.path.insert(0, str(_SDK_ROOT))

from osb_engine_sdk.worker import BaseEngineWorker  # noqa: E402

KOKORO_SAMPLE_RATE = 24000


def _lang_to_kokoro_code(lang: str, fallback: str) -> str:
    norm = lang.strip().lower()
    if norm.startswith("en-gb"):
        return "b"
    if norm.startswith("en"):
        return "a"
    if norm.startswith("pt"):
        return "p"
    return fallback


class KokoroTtsWorker(BaseEngineWorker):
    """Persistent Kokoro-82M TTS worker speaking OSBP v1 over Unix Domain Sockets."""

    def __init__(
        self,
        engine_id: str = "kokoro-tts",
        default_voice: str = "af_heart",
        lang_code: str = "a",
        repo_id: str = "hexgrad/Kokoro-82M",
        threads: int = 4,
        hf_home: str | None = None,
        offline: bool = True,
    ) -> None:
        super().__init__(max_in_flight=2)
        self.engine_id = engine_id
        self.default_voice = default_voice
        self.lang_code = lang_code
        self.repo_id = repo_id
        self.threads = max(1, threads)
        self.hf_home = hf_home
        self.offline = offline
        self._pipelines: dict[str, Any] = {}

    def engine_info(self) -> dict[str, Any]:
        return {
            "id": self.engine_id,
            "name": "Kokoro-82M CPU TTS",
            "version": "0.1.0",
            "engine_type": "python",
            "capabilities": {
                "capabilities": ["tts", "cpu_only"],
                "language_pairs": [],
                "stt_languages": [],
                "tts_languages": ["en-US", "en-GB", "pt-BR"],
            },
            "model": self.repo_id,
            "license": "Apache-2.0",
            "weights_license": "Apache-2.0 (verify individual voice pack provenance before redistribution)",
            "commercial_use": True,
        }

    def on_initialize(self, config: Any) -> None:
        del config
        if self.hf_home and os.path.isdir(self.hf_home):
            os.environ["HF_HOME"] = self.hf_home
        else:
            local_hf = _SDK_ROOT.parents[1] / "tools" / "benchmark" / ".cache" / "huggingface"
            if local_hf.is_dir():
                os.environ.setdefault("HF_HOME", str(local_hf))
        if self.offline:
            os.environ.setdefault("HF_HUB_OFFLINE", "1")

        import torch

        torch.set_num_threads(self.threads)
        try:
            torch.set_num_interop_threads(1)
        except RuntimeError:
            # PyTorch raises if interop threads were already configured once in-process.
            pass

        self._get_pipeline(self.lang_code)

    def _get_pipeline(self, code: str) -> Any:
        if code not in self._pipelines:
            from kokoro import KPipeline

            self._pipelines[code] = KPipeline(
                lang_code=code,
                device="cpu",
                repo_id=self.repo_id,
            )
        return self._pipelines[code]

    def process_tts(self, request: dict[str, Any]) -> tuple[dict[str, Any], bytes]:
        import numpy as np

        req_id = int(request["request_id"])
        text = str(request.get("text", "")).strip()
        language = str(request.get("language", "en-US"))
        voice = str(request.get("voice_id") or self.default_voice)

        started = time.perf_counter()
        if not text:
            waveform = np.zeros(0, dtype="<f4")
        else:
            code = _lang_to_kokoro_code(language, self.lang_code)
            pipeline = self._get_pipeline(code)
            chunks = [
                np.asarray(audio, dtype="<f4")
                for _, _, audio in pipeline(text, voice=voice)
            ]
            waveform = (
                np.concatenate(chunks).astype("<f4", copy=False)
                if chunks
                else np.zeros(0, dtype="<f4")
            )

        elapsed_ms = int((time.perf_counter() - started) * 1000)
        duration_ms = int((waveform.size * 1000) // KOKORO_SAMPLE_RATE)
        pcm_bytes = waveform.tobytes()

        header = {
            "type": "tts_result",
            "request_id": req_id,
            "result": {
                "status": "success",
                "processing_ms": elapsed_ms,
                "data": {
                    "audio": [],
                    "sample_rate": KOKORO_SAMPLE_RATE,
                    "duration_ms": duration_ms,
                },
            },
        }
        return header, pcm_bytes


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="OpenSpeechBridge Kokoro-82M TTS UDS worker (OSBP v1)"
    )
    BaseEngineWorker.add_common_cli_args(parser)
    parser.add_argument("--engine-id", default="kokoro-tts")
    parser.add_argument("--voice", default="af_heart")
    parser.add_argument("--lang-code", default="a")
    parser.add_argument("--repo-id", default="hexgrad/Kokoro-82M")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--hf-home", default=None)
    parser.add_argument(
        "--allow-download",
        action="store_true",
        help="Allow downloading uncached HuggingFace weights over the network",
    )
    return parser


def main(argv: list[str] | None = None) -> None:
    args = build_arg_parser().parse_args(argv)
    worker = KokoroTtsWorker(
        engine_id=args.engine_id,
        default_voice=args.voice,
        lang_code=args.lang_code,
        repo_id=args.repo_id,
        threads=args.threads,
        hf_home=args.hf_home,
        offline=not args.allow_download,
    )
    worker.run_from_parsed_args(args)


if __name__ == "__main__":
    main()
