#!/usr/bin/env python3
"""Extract a deterministic, human-spoken PT-BR subset from FLEURS parquet."""

from __future__ import annotations

import argparse
import io
import json
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq
import soundfile as sf


def decode_audio(blob: dict) -> tuple[np.ndarray, int]:
    audio, rate = sf.read(io.BytesIO(blob["bytes"]), dtype="float32", always_2d=False)
    if audio.ndim > 1:
        audio = audio.mean(axis=1)
    return audio, rate


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--parquet", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument(
        "--references",
        type=Path,
        default=Path(__file__).parent / "texts" / "reference_translations.json",
    )
    args = parser.parse_args()

    references = json.loads(args.references.read_text(encoding="utf-8"))

    table = pq.read_table(
        args.parquet,
        columns=["id", "num_samples", "audio", "transcription", "raw_transcription", "gender"],
    )
    rows = table.to_pylist()
    # FLEURS pt_br validation has no clip below 5.04 s. Preserve complete
    # utterances instead of manufacturing truncated "short" sentences.
    short = sorted(
        (row for row in rows if row["num_samples"] / 16000 <= 7.0),
        key=lambda row: row["num_samples"],
    )[:10]
    medium = [row for row in rows if 7.0 < row["num_samples"] / 16000 <= 12.0][:10]
    if len(short) < 10 or len(medium) < 10:
        raise SystemExit(f"insufficient duration buckets: short={len(short)} medium={len(medium)}")

    args.output.mkdir(parents=True, exist_ok=True)
    samples: list[dict] = []
    decoded: list[tuple[np.ndarray, int]] = []
    for category, selected in (("short", short), ("medium", medium)):
        for index, row in enumerate(selected, start=1):
            audio, rate = decode_audio(row["audio"])
            if rate != 16000:
                raise SystemExit(f"unexpected FLEURS sample rate: {rate}")
            sample_id = f"{category}_{index:02d}"
            path = args.output / f"{sample_id}.wav"
            sf.write(path, audio, rate, subtype="PCM_16")
            decoded.append((audio, rate))
            samples.append(
                {
                    "id": sample_id,
                    "category": category,
                    "path": str(path),
                    "duration_seconds": round(len(audio) / rate, 6),
                    "pt_br": row["raw_transcription"],
                    "normalized_pt_br": row["transcription"],
                    "expected_en": references[row["raw_transcription"]],
                    "source_id": row["id"],
                    "gender_label": row["gender"],
                }
            )

    silence = np.zeros(int(0.25 * 16000), dtype=np.float32)
    long_parts: list[np.ndarray] = []
    long_text: list[str] = []
    total = 0
    for row, (audio, _) in zip(samples, decoded, strict=True):
        long_parts.extend((audio, silence))
        long_text.append(row["pt_br"])
        total += len(audio) + len(silence)
        if total / 16000 >= 60:
            break
    long_audio = np.concatenate(long_parts)
    long_path = args.output / "long_01.wav"
    sf.write(long_path, long_audio, 16000, subtype="PCM_16")
    samples.append(
        {
            "id": "long_01",
            "category": "long",
            "path": str(long_path),
            "duration_seconds": round(len(long_audio) / 16000, 6),
            "pt_br": " ".join(long_text),
            "normalized_pt_br": " ".join(long_text),
            "expected_en": " ".join(references[text] for text in long_text),
            "source_id": "composite of listed FLEURS validation samples",
            "gender_label": "mixed/sequence",
        }
    )

    manifest = {
        "dataset": "google/fleurs pt_br validation",
        "license": "CC-BY-4.0",
        "source": "https://huggingface.co/datasets/google/fleurs",
        "preprocessing": "decoded embedded audio; mono 16 kHz PCM_S16 WAV; no signal processing",
        "duration_note": "FLEURS pt_br validation minimum is 5.04 s; short bucket uses the ten complete shortest clips (~5-7 s), not truncated 2-5 s audio",
        "long_sample_note": "deterministic concatenation with 250 ms silence; human read speech, not a natural continuous monologue",
        "samples": samples,
    }
    args.manifest.parent.mkdir(parents=True, exist_ok=True)
    args.manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {len(samples)} samples to {args.output}")


if __name__ == "__main__":
    main()
