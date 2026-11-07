#!/usr/bin/env python3
"""Minimal offline PT-BR WAV -> English text -> English WAV proof."""

from __future__ import annotations

import argparse
import json
import os
import time
from pathlib import Path

ROOT = Path(__file__).parent
os.environ.setdefault("HF_HOME", str(ROOT / ".cache" / "huggingface"))
os.environ.setdefault("HF_HUB_OFFLINE", "1")

import numpy as np  # noqa: E402
import soundfile as sf  # noqa: E402
import torch  # noqa: E402
from faster_whisper import WhisperModel  # noqa: E402
from kokoro import KPipeline  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--summary", type=Path, required=True)
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--model", default="small")
    parser.add_argument("--voice", default="af_heart")
    args = parser.parse_args()

    torch.set_num_threads(args.threads)
    torch.set_num_interop_threads(1)
    args.output_dir.mkdir(parents=True, exist_ok=True)

    started = time.perf_counter()
    whisper = WhisperModel(
        args.model,
        device="cpu",
        compute_type="int8",
        cpu_threads=args.threads,
        num_workers=1,
        download_root=str(ROOT / ".cache" / f"faster-whisper-{args.model}"),
    )
    whisper_load = time.perf_counter() - started

    started = time.perf_counter()
    segments, info = whisper.transcribe(
        str(args.input),
        task="translate",
        language="pt",
        beam_size=5,
        best_of=5,
        temperature=0.0,
        vad_filter=False,
        condition_on_previous_text=False,
    )
    translated = " ".join(segment.text.strip() for segment in segments).strip()
    translation_seconds = time.perf_counter() - started

    started = time.perf_counter()
    kokoro = KPipeline(lang_code="a", device="cpu", repo_id="hexgrad/Kokoro-82M")
    kokoro_load = time.perf_counter() - started

    started = time.perf_counter()
    chunks = [np.asarray(audio, dtype=np.float32) for _, _, audio in kokoro(translated, voice=args.voice)]
    synthesis_seconds = time.perf_counter() - started
    waveform = np.concatenate(chunks)

    text_path = args.output_dir / "translated.txt"
    wav_path = args.output_dir / "output.wav"
    text_path.write_text(translated + "\n", encoding="utf-8")
    sf.write(wav_path, waveform, 24000, subtype="PCM_16")
    result = {
        "status": "pass",
        "input": str(args.input),
        "whisper_engine": "faster-whisper",
        "whisper_model": args.model,
        "compute_type": "int8",
        "threads": args.threads,
        "detected_language": info.language,
        "translated_text": translated,
        "whisper_load_seconds": whisper_load,
        "translation_seconds": translation_seconds,
        "kokoro_load_seconds": kokoro_load,
        "synthesis_seconds": synthesis_seconds,
        "output_audio_seconds": len(waveform) / 24000,
        "output_sample_rate": 24000,
        "text_output": str(text_path),
        "wav_output": str(wav_path),
    }
    args.summary.parent.mkdir(parents=True, exist_ok=True)
    args.summary.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
