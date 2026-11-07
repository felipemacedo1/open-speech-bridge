#!/usr/bin/env python3
"""Aggregate JSONL benchmark records into a single CSV and summary JSON."""

from __future__ import annotations

import argparse
import csv
import json
import statistics
from collections import defaultdict
from pathlib import Path


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, round((len(ordered) - 1) * fraction))]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("inputs", nargs="+", type=Path)
    parser.add_argument("--csv", type=Path, required=True)
    parser.add_argument("--summary", type=Path, required=True)
    args = parser.parse_args()

    rows = [json.loads(line) for path in args.inputs for line in path.read_text(encoding="utf-8").splitlines() if line]
    columns = sorted({key for row in rows for key in row})
    args.csv.parent.mkdir(parents=True, exist_ok=True)
    with args.csv.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=columns, extrasaction="ignore")
        writer.writeheader()
        writer.writerows(rows)

    grouped: dict[str, list[dict]] = defaultdict(list)
    for row in rows:
        key = "/".join(str(row.get(field, "")) for field in ("engine", "model", "compute_type", "threads"))
        grouped[key].append(row)
    summary = {}
    for key, records in grouped.items():
        rtfs = [record["rtf"] for record in records if record.get("rtf") is not None]
        inference = [record.get("inference_seconds", record.get("synthesis_seconds")) for record in records]
        inference = [value for value in inference if value is not None]
        summary[key] = {
            "records": len(records),
            "median_rtf": statistics.median(rtfs),
            "p95_rtf": percentile(rtfs, 0.95),
            "median_inference_seconds": statistics.median(inference),
            "p95_inference_seconds": percentile(inference, 0.95),
            "peak_rss_bytes": max(record.get("peak_rss_bytes", 0) for record in records),
            "model_load_seconds": max(record.get("model_load_seconds") or 0 for record in records),
        }
    args.summary.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
