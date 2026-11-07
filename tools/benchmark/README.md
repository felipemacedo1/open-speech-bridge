# M2 CPU Inference Benchmark

Isolated research tooling for direct PT-BR speech-to-English translation and
Kokoro English synthesis. Nothing here is connected to PipeWire or the Rust
runtime.

## Fixed benchmark matrix

- whisper.cpp v1.9.4, CPU-only, `base-q5_1` and `small-q5_1`.
- faster-whisper 1.2.1 / CTranslate2 4.8.2, CPU INT8, `base` and `small`.
- Four inference threads (physical-core count) for the complete corpus. Eight
  threads are measured only on the long sample as a scaling check.
- Kokoro 0.9.4 / Kokoro-82M, `af_heart`, four CPU threads.
- PyTorch 2.6.0 CPU-only and Transformers 4.48.3, matching Kokoro's official
  lockfile and avoiding CUDA packages or an untested Transformers major release.
- Beam size and best-of are both five for the two Whisper implementations.

Tiny was considered but excluded from the measurement matrix: it saves little
enough time on this four-core machine to justify using the lowest-capacity model
as the product foundation before base/small quality is known. No medium/large
model is downloaded.

## Local storage policy

`.venv`, `.cache`, `corpus`, model files and generated WAV files are ignored by
Git. The committed results contain measurements and text only. FLEURS audio is
CC-BY-4.0 and remains a local benchmark artifact.

## Setup

```bash
./tools/benchmark/setup_python.sh
./tools/benchmark/setup_whisper_cpp.sh
tools/benchmark/.venv/bin/python tools/benchmark/prefetch_models.py all
```

Download only the FLEURS PT-BR validation parquet:

```bash
curl -L \
  -o tools/benchmark/.cache/fleurs-pt_br-validation.parquet \
  https://huggingface.co/datasets/google/fleurs/resolve/main/parquet-data/pt_br/validation-00000-of-00001.parquet

tools/benchmark/.venv/bin/python tools/benchmark/fetch_fleurs_subset.py \
  --parquet tools/benchmark/.cache/fleurs-pt_br-validation.parquet \
  --output tools/benchmark/corpus \
  --manifest tools/benchmark/corpus/manifest.json
```

The extractor creates the ten shortest complete samples (~5–7 s), ten 7–12 s
samples and a deterministic ~60 s concatenation with 250 ms silences. FLEURS
PT-BR validation has no 2–5 s clip (minimum 5.04 s), so the tooling preserves
complete utterances instead of manufacturing truncated “short” sentences. It
does not synthesize Portuguese.

## Run

Examples for the four-thread complete corpus:

```bash
tools/benchmark/.venv/bin/python tools/benchmark/benchmark_whisper_cpp.py \
  --cli tools/benchmark/.cache/whisper.cpp/build/bin/whisper-cli \
  --model tools/benchmark/.cache/models/ggml-base-q5_1.bin \
  --model-name base-q5_1 --engine-version v1.9.4 --threads 4 \
  --manifest tools/benchmark/corpus/manifest.json \
  --output tools/benchmark/results/whisper-cpp-base-q5_1-t4.jsonl

tools/benchmark/.venv/bin/python tools/benchmark/benchmark_faster_whisper.py \
  --model base --compute-type int8 --threads 4 --workers 1 \
  --cache tools/benchmark/.cache/faster-whisper-base \
  --manifest tools/benchmark/corpus/manifest.json \
  --output tools/benchmark/results/faster-whisper-base-int8-t4.jsonl

tools/benchmark/.venv/bin/python tools/benchmark/benchmark_kokoro.py \
  --texts tools/benchmark/texts/kokoro_cases.json \
  --output-dir tools/benchmark/generated --threads 4 \
  --output tools/benchmark/results/kokoro-af_heart-t4.jsonl

tools/benchmark/.venv/bin/python tools/benchmark/run_offline_pt_to_en.py \
  tools/benchmark/corpus/short_05.wav \
  --output-dir tools/benchmark/generated/offline-spike \
  --summary tools/benchmark/results/offline-spike.json
```

Use `--only-category long --threads 8` for the eight-thread scaling run. Results
can be combined with:

```bash
tools/benchmark/.venv/bin/python tools/benchmark/aggregate_results.py \
  tools/benchmark/results/*.jsonl \
  --csv tools/benchmark/results/benchmark-results.csv \
  --summary tools/benchmark/results/benchmark-summary.json
```

## Measurement semantics

- faster-whisper is loaded once per configuration. All but the first utterance
  are true warm inferences.
- whisper.cpp v1.9.4 CLI is a fresh process per utterance. Its own reported
  model-load time is subtracted from wall time for the warm estimate; this is
  labeled explicitly rather than represented as streaming latency.
- CPU percentages may exceed 100% because they are summed across cores.
- Preprocessing/extraction is outside inference timing.
- Batch timings are not time-to-first-partial measurements.
