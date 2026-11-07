#!/usr/bin/env python3
"""Benchmark whisper.cpp direct PT-BR speech-to-English translation on CPU."""

from __future__ import annotations

import argparse
import json
import os
import re
import tempfile
from pathlib import Path

from common import append_jsonl, load_manifest, run_sampled, wav_duration


LOAD_TIME = re.compile(r"load time\s*=\s*([0-9.]+)\s*ms")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--model-name", required=True)
    parser.add_argument("--threads", type=int, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--engine-version", required=True)
    parser.add_argument("--only-category", choices=("short", "medium", "long"))
    args = parser.parse_args()

    args.output.unlink(missing_ok=True)
    samples = load_manifest(args.manifest)
    if args.only_category:
        samples = [sample for sample in samples if sample["category"] == args.only_category]
    env = os.environ.copy()
    env["GGML_CUDA_DISABLE"] = "1"

    for index, sample in enumerate(samples):
        audio_path = Path(sample["path"])
        with tempfile.TemporaryDirectory(prefix="osb-whisper-cpp-") as directory:
            prefix = Path(directory) / "translation"
            command = [
                str(args.cli),
                "-m", str(args.model),
                "-f", str(audio_path),
                "-l", "pt",
                "-tr",
                "-t", str(args.threads),
                "-ng",
                "-bs", "5",
                "-bo", "5",
                "-otxt",
                "-of", str(prefix),
            ]
            completed, wall_seconds, stats = run_sampled(command, env=env)
            if completed.returncode != 0:
                raise SystemExit(f"whisper.cpp failed for {sample['id']}:\n{completed.stderr}")
            text_path = Path(str(prefix) + ".txt")
            output_text = text_path.read_text(encoding="utf-8").strip()
            timing_text = completed.stderr + "\n" + completed.stdout
            load_match = LOAD_TIME.search(timing_text)
            load_seconds = float(load_match.group(1)) / 1000 if load_match else None
            inference_seconds = wall_seconds - load_seconds if load_seconds is not None else wall_seconds
            duration = wav_duration(audio_path)
            append_jsonl(
                args.output,
                {
                    "engine": "whisper.cpp",
                    "engine_version": args.engine_version,
                    "device": "cpu",
                    "model": args.model_name,
                    "model_path": str(args.model),
                    "model_bytes": args.model.stat().st_size,
                    "quantization": args.model_name.rsplit("-", 1)[-1],
                    "threads": args.threads,
                    "sample_id": sample["id"],
                    "category": sample["category"],
                    "audio_path": str(audio_path),
                    "audio_duration_seconds": duration,
                    "model_load_seconds": load_seconds,
                    "cold_load_plus_first_inference_seconds": wall_seconds if index == 0 else None,
                    "inference_seconds": inference_seconds,
                    "process_wall_seconds": wall_seconds,
                    "rtf": inference_seconds / duration,
                    "peak_rss_bytes": stats.peak_rss_bytes,
                    "average_cpu_percent": stats.average_cpu_percent,
                    "peak_cpu_percent": stats.peak_cpu_percent,
                    "detected_language": "pt (forced)",
                    "output_text": output_text,
                    "expected_en": sample.get("expected_en", ""),
                    "pt_br": sample.get("pt_br", ""),
                    "run_kind": "first" if index == 0 else "warm-estimate",
                    "warm_method_note": "new CLI process per sample; load time reported by whisper.cpp subtracted from wall time",
                },
            )
            print(json.dumps({"sample": sample["id"], "seconds": inference_seconds, "rtf": inference_seconds / duration, "text": output_text}, ensure_ascii=False))


if __name__ == "__main__":
    main()
