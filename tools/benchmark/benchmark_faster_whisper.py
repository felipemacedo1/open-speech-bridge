#!/usr/bin/env python3
"""Benchmark faster-whisper direct PT-BR speech-to-English translation on CPU."""

from __future__ import annotations

import argparse
import json
import os
import time
from importlib.metadata import version
from pathlib import Path

import ctranslate2
from faster_whisper import WhisperModel

from common import append_jsonl, directory_size, load_manifest, sample_current_process, wav_duration


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", choices=("base", "small"), required=True)
    parser.add_argument("--compute-type", default="int8")
    parser.add_argument("--threads", type=int, required=True)
    parser.add_argument("--workers", type=int, default=1)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--only-category", choices=("short", "medium", "long"))
    args = parser.parse_args()

    os.environ["OMP_NUM_THREADS"] = str(args.threads)
    args.output.unlink(missing_ok=True)
    with sample_current_process() as load_resources:
        started = time.perf_counter()
        model = WhisperModel(
            args.model,
            device="cpu",
            compute_type=args.compute_type,
            cpu_threads=args.threads,
            num_workers=args.workers,
            download_root=str(args.cache),
        )
        load_seconds = time.perf_counter() - started
    load_stats = load_resources["stats"]

    samples = load_manifest(args.manifest)
    if args.only_category:
        samples = [sample for sample in samples if sample["category"] == args.only_category]

    for index, sample in enumerate(samples):
        audio_path = Path(sample["path"])
        with sample_current_process() as infer_resources:
            started = time.perf_counter()
            segments, info = model.transcribe(
                str(audio_path),
                task="translate",
                language="pt",
                beam_size=5,
                best_of=5,
                temperature=0.0,
                vad_filter=False,
                condition_on_previous_text=False,
            )
            text = " ".join(segment.text.strip() for segment in segments).strip()
            inference_seconds = time.perf_counter() - started
        infer_stats = infer_resources["stats"]
        duration = wav_duration(audio_path)
        append_jsonl(
            args.output,
            {
                "engine": "faster-whisper",
                "engine_version": version("faster-whisper"),
                "runtime": "CTranslate2",
                "runtime_version": ctranslate2.__version__,
                "device": "cpu",
                "model": args.model,
                "compute_type": args.compute_type,
                "threads": args.threads,
                "workers": args.workers,
                "sample_id": sample["id"],
                "category": sample["category"],
                "audio_path": str(audio_path),
                "audio_duration_seconds": duration,
                "model_load_seconds": load_seconds,
                "cold_load_plus_first_inference_seconds": load_seconds + inference_seconds if index == 0 else None,
                "inference_seconds": inference_seconds,
                "rtf": inference_seconds / duration,
                "peak_rss_bytes": max(load_stats.peak_rss_bytes, infer_stats.peak_rss_bytes),
                "inference_peak_rss_bytes": infer_stats.peak_rss_bytes,
                "average_cpu_percent": infer_stats.average_cpu_percent,
                "peak_cpu_percent": infer_stats.peak_cpu_percent,
                "detected_language": info.language,
                "language_probability": info.language_probability,
                "output_text": text,
                "model_cache_bytes": directory_size(args.cache),
                "expected_en": sample.get("expected_en", ""),
                "pt_br": sample.get("pt_br", ""),
                "run_kind": "first" if index == 0 else "warm",
            },
        )
        print(json.dumps({"sample": sample["id"], "seconds": inference_seconds, "rtf": inference_seconds / duration, "text": text}, ensure_ascii=False))


if __name__ == "__main__":
    main()
