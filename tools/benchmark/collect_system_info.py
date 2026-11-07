#!/usr/bin/env python3
"""Collect the reproducibility metadata for a benchmark run."""

from __future__ import annotations

import argparse
import json
import platform
import shutil
import subprocess
from pathlib import Path

import psutil


def read_optional(path: str) -> str | None:
    candidate = Path(path)
    return candidate.read_text(encoding="utf-8").strip() if candidate.is_file() else None


def command(*args: str) -> str:
    return subprocess.run(args, check=False, capture_output=True, text=True).stdout.strip()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    lscpu = command("lscpu")
    flags_line = next((line for line in lscpu.splitlines() if line.startswith("Flags:")), "")
    flags = set(flags_line.partition(":")[2].split())
    disk = shutil.disk_usage(Path.cwd())
    info = {
        "os": platform.platform(),
        "distribution": command("bash", "-lc", ". /etc/os-release && printf '%s %s' \"$PRETTY_NAME\" \"$VERSION_CODENAME\""),
        "kernel": platform.release(),
        "architecture": platform.machine(),
        "cpu_model": next((line.partition(":")[2].strip() for line in lscpu.splitlines() if line.startswith("Model name:")), "unknown"),
        "physical_cores": psutil.cpu_count(logical=False),
        "logical_cores": psutil.cpu_count(logical=True),
        "ram_bytes": psutil.virtual_memory().total,
        "simd": {name: name in flags for name in ("sse4_1", "sse4_2", "avx", "avx2", "avx512f", "fma")},
        "python": platform.python_version(),
        "rustc": command("rustc", "--version"),
        "cargo": command("cargo", "--version"),
        "compiler": command("cc", "--version").splitlines()[0],
        "disk_free_bytes": disk.free,
        "gpu": command("bash", "-lc", "lspci | grep -Ei 'VGA|3D|Display'") or "none detected",
        "power": {
            "ac_online": read_optional("/sys/class/power_supply/ADP1/online"),
            "battery_percent": read_optional("/sys/class/power_supply/BAT0/capacity"),
            "battery_status": read_optional("/sys/class/power_supply/BAT0/status"),
            "cpu_governor": read_optional("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
            "intel_pstate": read_optional("/sys/devices/system/cpu/intel_pstate/status"),
            "turbo_disabled": read_optional("/sys/devices/system/cpu/intel_pstate/no_turbo"),
        },
        "benchmark_policy": "CPU only; no CUDA, Vulkan, OpenVINO, Metal or ROCm",
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(info, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(info, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
