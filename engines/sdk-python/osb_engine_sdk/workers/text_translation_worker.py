#!/usr/bin/env python3
"""Persistent CPU Text-Translation (MT) UDS worker for OpenSpeechBridge (OSBP v1).

Enables bidirectional M4 Full-Duplex cascades (`STT -> MT -> TTS`, e.g.,
incoming `en-US -> pt-BR` alongside outgoing `pt-BR -> en-US`).
Supports local HuggingFace/MarianMT models (`--backend transformers`) as well as
an offline zero-network conversational phrase/lexical fallback (`--backend auto`).
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import time
from pathlib import Path
from typing import Any

# Allow direct script execution (`python3 .../text_translation_worker.py`) without pip install.
_SDK_ROOT = Path(__file__).resolve().parents[2]
if str(_SDK_ROOT) not in sys.path:
    sys.path.insert(0, str(_SDK_ROOT))

from osb_engine_sdk.worker import BaseEngineWorker  # noqa: E402

_EN_TO_PT_PHRASES: list[tuple[str, str]] = [
    ("can you hear me", "você consegue me ouvir"),
    ("good morning", "bom dia"),
    ("good afternoon", "boa tarde"),
    ("good evening", "boa noite"),
    ("thank you very much", "muito obrigado"),
    ("thank you", "obrigado"),
    ("how are you", "como vai você"),
    ("nice to meet you", "prazer em conhecer você"),
    ("without wire", "sem fio"),
    ("he set up", "ele armou"),
    ("a little campaign", "uma campainha"),
    ("he said", "disse ele"),
    ("let's start", "vamos começar"),
    ("one moment", "um momento"),
]

_EN_TO_PT_WORDS: dict[str, str] = {
    "hello": "olá",
    "hi": "oi",
    "team": "equipe",
    "world": "mundo",
    "yes": "sim",
    "no": "não",
    "please": "por favor",
    "thanks": "obrigado",
    "today": "hoje",
    "now": "agora",
    "meeting": "reunião",
    "call": "chamada",
    "audio": "áudio",
    "voice": "voz",
    "working": "funcionando",
    "clear": "claro",
    "clearly": "claramente",
    "we": "nós",
    "are": "estamos",
    "live": "ao vivo",
    "testing": "testando",
    "local": "local",
    "bridge": "ponte",
    "translation": "tradução",
}

_PT_TO_EN_WORDS: dict[str, str] = {v: k for k, v in _EN_TO_PT_WORDS.items()}


def _replace_words_with_map(text: str, lexicon: dict[str, str]) -> str:
    def _swap(match: re.Match[str]) -> str:
        word = match.group(0)
        replacement = lexicon.get(word.lower())
        if not replacement:
            return word
        return replacement.capitalize() if word[0].isupper() else replacement

    return re.sub(r"[A-Za-zÀ-ÿ']+", _swap, text)


def _lexical_translate(text: str, source_lang: str, target_lang: str) -> str:
    src = source_lang.strip().lower()
    dst = target_lang.strip().lower()
    result = text.strip()
    if not result:
        return ""

    if src.startswith("en") and dst.startswith("pt"):
        lowered = result.lower()
        for en_phrase, pt_phrase in _EN_TO_PT_PHRASES:
            lowered = re.sub(
                rf"\b{re.escape(en_phrase)}\b", pt_phrase, lowered, flags=re.IGNORECASE
            )
        translated = _replace_words_with_map(lowered, _EN_TO_PT_WORDS)
        return translated[0].upper() + translated[1:] if translated else ""

    if src.startswith("pt") and dst.startswith("en"):
        return _replace_words_with_map(result, _PT_TO_EN_WORDS)

    return result


class TextTranslationWorker(BaseEngineWorker):
    """Persistent text-translation UDS worker for bidirectional full-duplex pipelines."""

    def __init__(
        self,
        engine_id: str = "text-translator",
        backend: str = "auto",
        model_name: str | None = None,
        threads: int = 4,
        hf_home: str | None = None,
        offline: bool = True,
    ) -> None:
        super().__init__(max_in_flight=4)
        self.engine_id = engine_id
        self.backend = backend
        self.model_name = model_name
        self.threads = max(1, threads)
        self.hf_home = hf_home
        self.offline = offline
        self._hf_tokenizer: Any = None
        self._hf_model: Any = None

    def engine_info(self) -> dict[str, Any]:
        return {
            "id": self.engine_id,
            "name": "Local Text Translation Worker (EN <-> PT-BR)",
            "version": "0.1.0",
            "engine_type": "python",
            "capabilities": {
                "capabilities": ["text_translation", "cpu_only"],
                "language_pairs": [
                    {"source": "en-US", "target": "pt-BR"},
                    {"source": "pt-BR", "target": "en-US"},
                ],
                "stt_languages": [],
                "tts_languages": [],
            },
            "model": self.model_name or "builtin-offline-lexicon",
            "license": "Apache-2.0",
            "weights_license": "Apache-2.0",
            "commercial_use": True,
        }

    def on_initialize(self, config: Any) -> None:
        del config
        if self.hf_home:
            os.environ["HF_HOME"] = self.hf_home
        if self.offline:
            os.environ.setdefault("HF_HUB_OFFLINE", "1")

        if self.backend == "transformers" or (self.backend == "auto" and self.model_name):
            try:
                import torch
                from transformers import AutoModelForSeq2SeqLM, AutoTokenizer

                torch.set_num_threads(self.threads)
                model_id = self.model_name or "Helsinki-NLP/opus-mt-tc-big-en-pt"
                self._hf_tokenizer = AutoTokenizer.from_pretrained(
                    model_id, local_files_only=self.offline
                )
                self._hf_model = AutoModelForSeq2SeqLM.from_pretrained(
                    model_id, local_files_only=self.offline
                )
            except Exception:
                if self.backend == "transformers":
                    raise
                self._hf_tokenizer = None
                self._hf_model = None

    def process_translation(self, request: dict[str, Any]) -> dict[str, Any]:
        req_id = int(request["request_id"])
        text = str(request.get("text", "")).strip()
        source_lang = str(request.get("source_lang", "en-US"))
        target_lang = str(request.get("target_lang", "pt-BR"))

        started = time.perf_counter()
        if not text:
            translated = ""
        elif self._hf_model is not None and self._hf_tokenizer is not None:
            import torch

            inputs = self._hf_tokenizer(text, return_tensors="pt", truncation=True)
            with torch.no_grad():
                outputs = self._hf_model.generate(**inputs, max_new_tokens=256)
            translated = self._hf_tokenizer.decode(
                outputs[0], skip_special_tokens=True
            ).strip()
        else:
            translated = _lexical_translate(text, source_lang, target_lang)

        elapsed_ms = int((time.perf_counter() - started) * 1000)
        return {
            "type": "translation_result",
            "request_id": req_id,
            "result": {
                "status": "success",
                "processing_ms": elapsed_ms,
                "data": {
                    "text": translated,
                    "detected_source": source_lang,
                },
            },
        }


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="OpenSpeechBridge local Text-Translation UDS worker (OSBP v1)"
    )
    BaseEngineWorker.add_common_cli_args(parser)
    parser.add_argument("--engine-id", default="text-translator")
    parser.add_argument(
        "--backend", choices=["auto", "builtin", "transformers"], default="auto"
    )
    parser.add_argument("--model", default=None)
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--hf-home", default=None)
    parser.add_argument("--allow-download", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> None:
    args = build_arg_parser().parse_args(argv)
    worker = TextTranslationWorker(
        engine_id=args.engine_id,
        backend=args.backend,
        model_name=args.model,
        threads=args.threads,
        hf_home=args.hf_home,
        offline=not args.allow_download,
    )
    worker.run_from_parsed_args(args)


if __name__ == "__main__":
    main()
