#!/usr/bin/env python3
"""Populate local benchmark caches before timed runs."""

from __future__ import annotations

import argparse
import os
from pathlib import Path

ROOT = Path(__file__).parent
os.environ.setdefault("HF_HOME", str(ROOT / ".cache" / "huggingface"))

from faster_whisper import WhisperModel  # noqa: E402
from huggingface_hub import hf_hub_download  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("component", choices=("faster-whisper", "kokoro", "all"))
    args = parser.parse_args()

    if args.component in ("faster-whisper", "all"):
        for model in ("base", "small"):
            WhisperModel(
                model,
                device="cpu",
                compute_type="int8",
                cpu_threads=4,
                download_root=str(ROOT / ".cache" / f"faster-whisper-{model}"),
            )
    if args.component in ("kokoro", "all"):
        for filename in ("config.json", "kokoro-v1_0.pth", "voices/af_heart.pt"):
            hf_hub_download(repo_id="hexgrad/Kokoro-82M", filename=filename)

    print(f"cached {args.component} benchmark artifacts")


if __name__ == "__main__":
    main()
