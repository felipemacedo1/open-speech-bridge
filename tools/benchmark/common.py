"""Shared helpers for the isolated CPU inference benchmark."""

from __future__ import annotations

import contextlib
import json
import os
import subprocess
import threading
import time
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterator

import psutil


def load_manifest(path: Path) -> list[dict[str, Any]]:
    data = json.loads(path.read_text(encoding="utf-8"))
    return data["samples"]


def wav_duration(path: Path) -> float:
    with wave.open(str(path), "rb") as handle:
        return handle.getnframes() / handle.getframerate()


def directory_size(path: Path) -> int:
    if path.is_file():
        return path.stat().st_size
    if not path.exists():
        return 0
    return sum(item.stat().st_size for item in path.rglob("*") if item.is_file())


def append_jsonl(path: Path, record: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n")


@dataclass
class ResourceStats:
    peak_rss_bytes: int = 0
    average_cpu_percent: float = 0.0
    peak_cpu_percent: float = 0.0


class ProcessSampler:
    """Sample RSS and CPU for a process tree without affecting its output."""

    def __init__(self, pid: int, interval: float = 0.05) -> None:
        self._process = psutil.Process(pid)
        self._interval = interval
        self._stop = threading.Event()
        self._samples: list[float] = []
        self._peak_rss = 0
        self._thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self._prime_cpu()
        self._thread.start()

    def stop(self) -> ResourceStats:
        self._stop.set()
        self._thread.join(timeout=2)
        return ResourceStats(
            peak_rss_bytes=self._peak_rss,
            average_cpu_percent=(sum(self._samples) / len(self._samples)) if self._samples else 0.0,
            peak_cpu_percent=max(self._samples, default=0.0),
        )

    def _tree(self) -> list[psutil.Process]:
        try:
            return [self._process, *self._process.children(recursive=True)]
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            return []

    def _prime_cpu(self) -> None:
        for process in self._tree():
            with contextlib.suppress(psutil.NoSuchProcess, psutil.AccessDenied):
                process.cpu_percent(None)

    def _run(self) -> None:
        while not self._stop.wait(self._interval):
            rss = 0
            cpu = 0.0
            for process in self._tree():
                with contextlib.suppress(psutil.NoSuchProcess, psutil.AccessDenied):
                    rss += process.memory_info().rss
                    cpu += process.cpu_percent(None)
            self._peak_rss = max(self._peak_rss, rss)
            self._samples.append(cpu)


@contextlib.contextmanager
def sample_current_process() -> Iterator[dict[str, ResourceStats]]:
    sampler = ProcessSampler(os.getpid())
    sampler.start()
    result: dict[str, ResourceStats] = {}
    try:
        yield result
    finally:
        result["stats"] = sampler.stop()


def run_sampled(command: list[str], env: dict[str, str] | None = None) -> tuple[subprocess.CompletedProcess[str], float, ResourceStats]:
    started = time.perf_counter()
    process = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    sampler = ProcessSampler(process.pid)
    sampler.start()
    stdout, stderr = process.communicate()
    elapsed = time.perf_counter() - started
    stats = sampler.stop()
    completed = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
    return completed, elapsed, stats
