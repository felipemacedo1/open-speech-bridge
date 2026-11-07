#!/usr/bin/env python3
"""Round-trip generated English WAVs through Whisper as a listening proxy."""

from __future__ import annotations

import argparse
import json
from difflib import SequenceMatcher
from pathlib import Path

from faster_whisper import WhisperModel


def normalized(text: str) -> str:
    return " ".join("".join(character.lower() if character.isalnum() else " " for character in text).split())


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--texts", type=Path, required=True)
    parser.add_argument("--audio-dir", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--threads", type=int, default=4)
    args = parser.parse_args()

    model = WhisperModel(
        "small",
        device="cpu",
        compute_type="int8",
        cpu_threads=args.threads,
        num_workers=1,
        download_root=str(args.cache),
    )
    cases = json.loads(args.texts.read_text(encoding="utf-8"))["cases"]
    records = []
    for case in cases:
        audio = args.audio_dir / f"kokoro_{case['id']}.wav"
        segments, _ = model.transcribe(str(audio), language="en", task="transcribe", beam_size=5, vad_filter=False)
        heard = " ".join(segment.text.strip() for segment in segments).strip()
        score = SequenceMatcher(None, normalized(case["text"]), normalized(heard)).ratio()
        records.append({"case_id": case["id"], "expected": case["text"], "round_trip_text": heard, "sequence_similarity": score})
        print(json.dumps(records[-1], ensure_ascii=False))
    args.output.write_text(json.dumps({"method": "faster-whisper small INT8 round trip; proxy only, not a human listening test", "records": records}, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
