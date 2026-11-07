#!/usr/bin/env python3
"""Persistent CPU/int8 faster-whisper UDS worker for OpenSpeechBridge (OSBP v1).

Supports both direct speech-to-text translation (`process_speech_translation`,
Whisper `task="translate"`) and speech recognition (`process_stt`,
Whisper `task="transcribe"`).
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from pathlib import Path
from typing import Any

# Allow direct script execution (`python3 .../faster_whisper_worker.py`) without pip install.
_SDK_ROOT = Path(__file__).resolve().parents[2]
if str(_SDK_ROOT) not in sys.path:
    sys.path.insert(0, str(_SDK_ROOT))

from osb_engine_sdk.framing import pcm_bytes_to_floats  # noqa: E402
from osb_engine_sdk.worker import BaseEngineWorker  # noqa: E402


def _normalize_whisper_lang(code: str | None) -> str | None:
    if not code:
        return None
    primary = code.strip().split("-")[0].split("_")[0].lower()
    return primary or None


class FasterWhisperWorker(BaseEngineWorker):
    """Persistent faster-whisper worker speaking OSBP v1 over Unix Domain Sockets."""

    def __init__(
        self,
        engine_id: str = "faster-whisper",
        model_name: str = "small",
        compute_type: str = "int8",
        threads: int = 4,
        beam_size: int = 5,
        download_root: str | None = None,
        offline: bool = True,
    ) -> None:
        super().__init__(max_in_flight=2)
        self.engine_id = engine_id
        self.model_name = model_name
        self.compute_type = compute_type
        self.threads = max(1, threads)
        self.beam_size = max(1, beam_size)
        self.download_root = download_root
        self.offline = offline
        self._model: Any = None

    def engine_info(self) -> dict[str, Any]:
        return {
            "id": self.engine_id,
            "name": f"Faster-Whisper ({self.model_name}, {self.compute_type})",
            "version": "0.1.0",
            "engine_type": "python",
            "capabilities": {
                "capabilities": ["stt", "speech_translation", "cpu_only"],
                "language_pairs": [
                    {"source": "pt-BR", "target": "en-US"},
                    {"source": "es-ES", "target": "en-US"},
                    {"source": "fr-FR", "target": "en-US"},
                    {"source": "de-DE", "target": "en-US"},
                    {"source": "it-IT", "target": "en-US"},
                ],
                "stt_languages": [
                    "pt-BR",
                    "en-US",
                    "es-ES",
                    "fr-FR",
                    "de-DE",
                    "it-IT",
                ],
                "tts_languages": [],
            },
            "model": self.model_name,
            "license": "MIT",
            "weights_license": "MIT",
            "commercial_use": True,
        }

    def on_initialize(self, config: Any) -> None:
        if self._model is not None:
            return
        if self.offline:
            os.environ.setdefault("HF_HUB_OFFLINE", "1")

        from faster_whisper import WhisperModel

        kwargs: dict[str, Any] = {
            "device": "cpu",
            "compute_type": self.compute_type,
            "cpu_threads": self.threads,
            "num_workers": 1,
            "local_files_only": self.offline,
        }
        if self.download_root and os.path.isdir(self.download_root):
            kwargs["download_root"] = self.download_root
        else:
            local_cache = _SDK_ROOT.parents[1] / "tools" / "benchmark" / ".cache" / f"faster-whisper-{self.model_name}"
            if local_cache.is_dir():
                kwargs["download_root"] = str(local_cache)

        self._model = WhisperModel(self.model_name, **kwargs)

    def _prepare_audio_16k(self, req: dict[str, Any], pcm_bytes: bytes) -> Any:
        import numpy as np

        if pcm_bytes:
            audio = np.frombuffer(pcm_bytes, dtype="<f4").astype(np.float32, copy=True)
        else:
            raw_list = req.get("audio") or []
            audio = np.asarray(raw_list, dtype=np.float32)

        sample_rate = int(req.get("sample_rate", 16000))
        if sample_rate != 16000 and audio.size > 1:
            target_len = max(1, int(round(audio.size * 16000.0 / sample_rate)))
            x_old = np.linspace(0.0, 1.0, num=audio.size, endpoint=False)
            x_new = np.linspace(0.0, 1.0, num=target_len, endpoint=False)
            audio = np.interp(x_new, x_old, audio).astype(np.float32)
        return audio

    def _run_whisper(
        self, audio: Any, task: str, source_lang: str | None
    ) -> tuple[str, str | None, float | None]:
        if self._model is None:
            self.on_initialize(None)

        segments, info = self._model.transcribe(
            audio,
            task=task,
            language=_normalize_whisper_lang(source_lang),
            beam_size=self.beam_size,
            best_of=self.beam_size,
            temperature=0.0,
            vad_filter=False,
            condition_on_previous_text=False,
        )
        text = " ".join(seg.text.strip() for seg in segments).strip()
        detected = getattr(info, "language", None)
        prob = getattr(info, "language_probability", None)
        conf = float(prob) if prob is not None else None
        return text, detected, conf

    def process_stt(
        self, request: dict[str, Any], pcm_bytes: bytes, flags: int
    ) -> dict[str, Any]:
        del flags
        req_id = int(request["request_id"])
        started = time.perf_counter()
        audio = self._prepare_audio_16k(request, pcm_bytes)
        if audio.size == 0:
            text, detected, conf = "", None, None
        else:
            text, detected, conf = self._run_whisper(audio, task="transcribe", source_lang=None)
        elapsed_ms = int((time.perf_counter() - started) * 1000)
        return {
            "type": "stt_result",
            "request_id": req_id,
            "result": {
                "status": "success",
                "processing_ms": elapsed_ms,
                "data": {
                    "text": text,
                    "language": detected,
                    "confidence": conf,
                    "words": None,
                },
            },
        }

    def process_speech_translation(
        self, request: dict[str, Any], pcm_bytes: bytes, flags: int
    ) -> dict[str, Any]:
        del flags
        req_id = int(request["request_id"])
        target_lang = str(request.get("target_lang", "en-US"))
        if _normalize_whisper_lang(target_lang) != "en":
            return {
                "type": "speech_translation_result",
                "request_id": req_id,
                "result": {
                    "status": "failed",
                    "code": "UNSUPPORTED_TARGET_LANGUAGE",
                    "message": f"Whisper translate only supports English target, got {target_lang}",
                },
            }

        started = time.perf_counter()
        audio = self._prepare_audio_16k(request, pcm_bytes)
        if audio.size == 0:
            text, detected, conf = "", None, None
        else:
            source_lang = request.get("source_lang")
            text, detected, conf = self._run_whisper(
                audio, task="translate", source_lang=source_lang
            )
        elapsed_ms = int((time.perf_counter() - started) * 1000)
        return {
            "type": "speech_translation_result",
            "request_id": req_id,
            "result": {
                "status": "success",
                "processing_ms": elapsed_ms,
                "data": {
                    "text": text,
                    "detected_source": detected,
                    "target_lang": target_lang,
                    "confidence": conf,
                },
            },
        }


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="OpenSpeechBridge faster-whisper UDS worker (OSBP v1)"
    )
    BaseEngineWorker.add_common_cli_args(parser)
    parser.add_argument("--engine-id", default="faster-whisper")
    parser.add_argument("--model", default="small")
    parser.add_argument("--compute-type", default="int8")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--beam-size", type=int, default=5)
    parser.add_argument("--download-root", default=None)
    parser.add_argument(
        "--allow-download",
        action="store_true",
        help="Allow downloading uncached HuggingFace weights over the network",
    )
    return parser


def main(argv: list[str] | None = None) -> None:
    _ = pcm_bytes_to_floats  # re-exported helper verification
    args = build_arg_parser().parse_args(argv)
    worker = FasterWhisperWorker(
        engine_id=args.engine_id,
        model_name=args.model,
        compute_type=args.compute_type,
        threads=args.threads,
        beam_size=args.beam_size,
        download_root=args.download_root,
        offline=not args.allow_download,
    )
    worker.run_from_parsed_args(args)


if __name__ == "__main__":
    main()
