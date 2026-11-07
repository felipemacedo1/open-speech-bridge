#!/usr/bin/env python3
"""Benchmark Kokoro-82M English synthesis on CPU."""

from __future__ import annotations

import argparse
import json
import os
import time
from importlib.metadata import version
from pathlib import Path

os.environ.setdefault("HF_HOME", str(Path(__file__).parent / ".cache" / "huggingface"))
os.environ.setdefault("HF_HUB_OFFLINE", "1")

import numpy as np
import soundfile as sf
import torch
from kokoro import KPipeline

from common import append_jsonl, sample_current_process


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--texts", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--threads", type=int, required=True)
    parser.add_argument("--voice", default="af_heart")
    args = parser.parse_args()

    torch.set_num_threads(args.threads)
    torch.set_num_interop_threads(1)
    args.output.unlink(missing_ok=True)
    args.output_dir.mkdir(parents=True, exist_ok=True)

    with sample_current_process() as load_resources:
        started = time.perf_counter()
        pipeline = KPipeline(lang_code="a", device="cpu")
        load_seconds = time.perf_counter() - started
    load_stats = load_resources["stats"]

    cases = json.loads(args.texts.read_text(encoding="utf-8"))["cases"]
    for index, case in enumerate(cases):
        chunks: list[np.ndarray] = []
        first_audio_seconds = None
        with sample_current_process() as synth_resources:
            started = time.perf_counter()
            generator = pipeline(case["text"], voice=args.voice, speed=1.0)
            for _, _, audio in generator:
                if first_audio_seconds is None:
                    first_audio_seconds = time.perf_counter() - started
                chunks.append(np.asarray(audio, dtype=np.float32))
            synthesis_seconds = time.perf_counter() - started
        stats = synth_resources["stats"]
        waveform = np.concatenate(chunks) if chunks else np.zeros(0, dtype=np.float32)
        output_path = args.output_dir / f"kokoro_{case['id']}.wav"
        sf.write(output_path, waveform, 24000, subtype="PCM_16")
        audio_duration = len(waveform) / 24000
        append_jsonl(
            args.output,
            {
                "engine": "kokoro",
                "engine_version": version("kokoro"),
                "model": "hexgrad/Kokoro-82M",
                "voice": args.voice,
                "device": "cpu",
                "threads": args.threads,
                "case_id": case["id"],
                "category": case["category"],
                "text": case["text"],
                "model_load_seconds": load_seconds,
                "cold_load_plus_first_synthesis_seconds": load_seconds + synthesis_seconds if index == 0 else None,
                "synthesis_seconds": synthesis_seconds,
                "time_to_first_audio_seconds": first_audio_seconds,
                "generated_audio_duration_seconds": audio_duration,
                "rtf": synthesis_seconds / audio_duration if audio_duration else None,
                "peak_rss_bytes": max(load_stats.peak_rss_bytes, stats.peak_rss_bytes),
                "synthesis_peak_rss_bytes": stats.peak_rss_bytes,
                "average_cpu_percent": stats.average_cpu_percent,
                "peak_cpu_percent": stats.peak_cpu_percent,
                "sample_rate": 24000,
                "format": "mono PCM_S16 WAV",
                "output_path": str(output_path),
                "peak_amplitude": float(np.max(np.abs(waveform))) if len(waveform) else 0.0,
                "rms": float(np.sqrt(np.mean(np.square(waveform)))) if len(waveform) else 0.0,
                "run_kind": "first" if index == 0 else "warm",
            },
        )
        print(json.dumps({"case": case["id"], "seconds": synthesis_seconds, "audio_seconds": audio_duration, "rtf": synthesis_seconds / audio_duration}, ensure_ascii=False))


if __name__ == "__main__":
    main()
