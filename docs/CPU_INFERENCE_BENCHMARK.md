# CPU Inference Benchmark

> Measurement date: 2026-09-27  
> Branch: `research/cpu-inference-benchmark`  
> Scope: offline research tooling only; no PipeWire, VAD, daemon, or production-provider integration

## Executive Summary

**Recommendation: use faster-whisper 1.2.1 / CTranslate2 4.8.2 with the
multilingual `small` model, CPU `int8`, four inference threads, and one
persistent worker as the provisional M2 foundation.** It gave materially better
PT-BR-to-English quality than `base` while remaining faster than real time on
this four-core laptop. It also beat whisper.cpp `small-q5_1` by a large margin
in this run, at the cost of a Python runtime, a larger model, and about 870 MiB
peak process RSS.

The selection is **provisional for latency**, not for basic functionality. The
machine was unplugged, at 16% battery, using the Linux `powersave` governor.
Sustained CPU performance fell by about 2x during the session. Kokoro ranged
from approximately 0.47 RTF in the earlier burst regime to 0.94 median RTF in
the committed sustained four-thread run. A short phrase was barely within 3 s
in the early regime before VAD/IPC overhead, but the measured sustained offline
spike needed 7.14 s for warm translation plus synthesis. A reliable 1.5–3 s
live claim is therefore **UNCERTAIN** until an AC-powered repeat and
sentence/incremental TTS experiment.

The end-to-end offline proof passed:

```text
FLEURS human PT-BR WAV
  -> faster-whisper small/int8 (direct translate)
  -> correct English text
  -> Kokoro-82M af_heart
  -> mono 24 kHz PCM WAV
```

No model or generated audio is committed. The small structured results and all
reproduction scripts are under [`tools/benchmark/`](../tools/benchmark/).

## Test Environment

| Item | Measured value |
|---|---|
| OS | Ubuntu 24.04.5 LTS (Noble), Linux 7.0.0-34-generic |
| Architecture | x86_64 |
| CPU | Intel Core i5-1135G7, 4 physical / 8 logical cores, up to 4.2 GHz |
| SIMD | SSE4.1, SSE4.2, AVX, AVX2, AVX-512F, FMA present |
| RAM | 15.35 GiB |
| Free disk at inventory | 125.9 GiB |
| GPU | Integrated Intel Iris Xe detected; not used |
| Python | 3.12.3 |
| Rust | rustc/cargo 1.98.1 |
| C compiler | GCC 13.3.0 |
| Power state | AC offline, battery 16% and discharging, `powersave`, Intel P-state active |

The scripts explicitly selected CPU runtimes. whisper.cpp was compiled with
CUDA, Vulkan and HIP disabled and invoked with `--no-gpu`; CTranslate2 used
`device="cpu"`/`int8`; PyTorch was `2.6.0+cpu` and reported CUDA unavailable.
The full machine record is in
[`system-info.json`](../tools/benchmark/results/system-info.json).

## Repository Baseline

Before adding benchmark tooling, the existing Docker development environment
passed:

| Gate | Baseline result |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo test --workspace` | PASS — 123 passed, 15 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `git diff --check` | PASS |

The checkout already contained the in-progress M1 PipeWire changes documented
in `docs/STATUS.md`; this task preserved them and made no production Rust/audio
change.

## Benchmark Methodology

- Direct Whisper translation only: forced Portuguese input, English output.
- Beam size 5, best-of 5, temperature 0, no VAD, no previous-text context.
- Complete matrix: `base` and `small`, four physical-core threads.
- Quantization: official whisper.cpp `q5_1`; CTranslate2 CPU `int8`.
- One 8-thread long-sample pass checked logical-core scaling; it was not an
  exhaustive tuning study.
- Models were cached before timed runs; download time is excluded.
- Resource sampling used the process tree every 50 ms. CPU percentage is summed
  across cores and can exceed 100%.
- faster-whisper loaded once per configuration; samples after the first are
  true warm inference.
- whisper.cpp's CLI creates a process for every sample. Its internal model-load
  time was subtracted from wall time, so its “warm” number is an estimate and
  still includes process/runtime setup. It is not streaming latency.
- Batch decoding does not expose a trustworthy first-partial-text measurement;
  none is claimed.
- Kokoro's generator time to first yielded audio is reported, but for short and
  medium inputs the first yield occurred only after the whole case was
  synthesized. It is not low-latency sample streaming.

The run order exposed strong battery/power throttling. Results are intentionally
not cherry-picked: the committed four-thread Kokoro data is the later sustained
run; the earlier four-thread run observed about 0.46–0.50 RTF, and the earlier
eight-thread artifact retained a comparable 0.47 median RTF. Thread-count
comparisons across those power regimes are not causal.

## Dataset

The repository had no reusable test corpus. A possibly personal recording was
found outside the repository and deliberately not used. The benchmark instead
uses the human-spoken Google FLEURS `pt_br` validation split, licensed
CC-BY-4.0 ([official dataset card](https://huggingface.co/datasets/google/fleurs)).

| Bucket | Count | Actual duration |
|---|---:|---:|
| Short complete utterances | 10 | 5.04–5.82 s |
| Medium | 10 | 7.44–11.94 s |
| Long composite | 1 | 68.57 s |

FLEURS PT-BR validation has no 2–5 s recording (minimum 5.04 s). The extractor
therefore takes the ten shortest **complete** human utterances rather than
truncating or synthesizing them. The long case deterministically concatenates
selected utterances with 250 ms silence; it is useful for throughput but is not
a natural one-minute monologue.

Embedded audio was decoded and written as mono 16 kHz PCM-S16 WAV without
signal processing. Extraction of all 21 local files took about 1.2 s and is
outside inference timing. Audio and the 282 MiB source parquet remain ignored;
the committed references were translated manually. FLEURS attribution must be
retained if the corpus is redistributed.

## whisper.cpp Setup

- Official [whisper.cpp](https://github.com/ggml-org/whisper.cpp) v1.9.4,
  commit `927cfce34f31707e17f2bff35c349632fb9e2c3a`.
- Release build, `GGML_NATIVE=ON`, OpenMP on, GPU backends off.
- `base-q5_1`: 59,707,625 bytes, SHA-256
  `422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898`.
- `small-q5_1`: 190,085,487 bytes, SHA-256
  `ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb`.

The q5_1 files came from the official downloader. The upstream release page
confirms v1.9.4 and its commit ([official releases](https://github.com/ggml-org/whisper.cpp/releases)).

## faster-whisper Setup

- faster-whisper 1.2.1 with CTranslate2 4.8.2.
- CPU `int8`, one worker, four inference threads.
- Converted `base` revision
  `ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b6`, 145,217,532-byte model.
- Converted `small` revision
  `536b0662742c02347bc0e980a01041f333bce120`, 483,546,902-byte model.

The upstream project explicitly recommends matching beam/thread settings for
comparisons and documents `OMP_NUM_THREADS` for CPU runs
([official faster-whisper README](https://github.com/SYSTRAN/faster-whisper)).

## Kokoro Setup

- `kokoro` Python package 0.9.4, PyTorch 2.6.0 CPU and Transformers 4.48.3.
- `hexgrad/Kokoro-82M`, revision
  `f3ff3571791e39611d31c381e3a41a3af07b4987`.
- `kokoro-v1_0.pth`: 327,212,226 bytes; model card SHA-256
  `496dba118d1a58f5f3db2efc88dbdc216e0483fc89fe6e47ee1f2c53f18ad1e4`.
- One voice only: `af_heart`, 523,425 bytes.
- Output: mono 24 kHz PCM-S16 WAV.
- The English G2P dependency's `en_core_web_sm` 3.8.0 model is pinned in the
  isolated benchmark requirements.

The official repository describes Kokoro as an 82M-parameter model with
Apache-licensed weights and demonstrates `af_heart`
([official Kokoro repository](https://github.com/hexgrad/kokoro),
[official model card](https://huggingface.co/hexgrad/Kokoro-82M)).

## Results

Four-thread full-corpus results below use warm medians (the first sample is
excluded where applicable). “Peak RAM” is measured process-tree RSS, not model
file size.

| Engine | Model | Quantization | Warm short median | Warm medium median | Long | Median RTF | Peak RAM | Load / first cold |
|---|---|---|---:|---:|---:|---:|---:|---:|
| whisper.cpp | base | q5_1 | 1.35 s | 1.63 s | 7.61 s | 0.184 | 258 MiB | 0.067 s median load / 4.15 s |
| whisper.cpp | small | q5_1 | 5.64 s | 10.72 s | 40.56 s | 0.992 | 529 MiB | 0.156 s median load / 4.67 s |
| faster-whisper | base | int8 | 0.65 s | 0.75 s | 2.66 s | 0.092 | 444 MiB | 0.49 s / 1.13 s |
| faster-whisper | small | int8 | 1.67 s | 1.93 s | 7.53 s | 0.239 | 870 MiB | 1.03 s / 2.74 s |

The whisper.cpp `small` full pass occurred later under the strongest measured
power throttling, so its absolute gap is not a clean library microbenchmark.
The earlier pass was roughly 4.6–6.6 s for short/medium clips and still much
slower than faster-whisper `small` at 1.65–2.13 s. Both implementations were
faster than real time on the long case in the earlier regime. Under sustained
battery load, whisper.cpp `small` crossed RTF 1 on several individual clips.

Eight logical threads did not produce a dependable product setting. For the
68.57 s sample, faster-whisper became slower in absolute time than its earlier
four-thread run (`small`: 12.78 s vs 7.53 s), while whisper.cpp `small` improved
relative to its later throttled full pass (19.63 s vs 40.56 s). Run order and
power state dominate these results. Four threads remains the conservative
default pending a controlled AC-powered sweep.

## Translation Quality

No automatic MT metric is promoted as the decision-maker: English translations
can be valid without matching one reference. Manual review found a consistent
capacity difference. `base` frequently lost content words (wireless doorbell,
Hebrew family life, beer, tail/plumage) and sometimes changed meaning. `small`
preserved most sentences, proper names and scores, but still made important
errors on acoustically difficult words.

| Input PT-BR | Expected English | whisper.cpp base | faster-whisper small | Comment |
|---|---|---|---|---|
| `Ele montou uma campainha sem fio, disse.` | He installed a wireless doorbell, he said. | He brought a little bell, said. | He set up a little campaign without wire, he said. | Both fail the key noun; `small` is not immune to acoustic ambiguity. |
| `A polícia disse que o corpo parecia estar ali há cerca de um dia.` | Police said the body appeared to have been there for about a day. | …there a few days ago. | …there for about a day. | `small` preserves the number/time meaning. |
| Local beer / Number One / Manta | Main local beer … pleasant and refreshing … | Repeatedly translated beer as “service/survey.” | Preserved beer, names and meaning. | Clear `small` advantage. |
| Final score `21 a 20` and 15 All Blacks wins | 21–20; ended 15-win streak. | Corrupted the ending. | Preserved score, team and streak. | Proper numbers/names pass on `small`. |
| Dinosaur tail / adult plumage / not down | Tail belonged to a young dinosaur; adult plumage, not down. | Severe noun errors. | Still changed “tail” and the final “down.” | Residual domain-vocabulary risk even with `small`. |

The long composite showed the same pattern and no extra hallucinated paragraph,
but repeated short-clip boundary context is artificial. PT-BR regional breadth,
spontaneous speech, crosstalk, noise and user-specific names remain untested.

## CPU and Memory

- faster-whisper `small/int8`: about 870 MiB peak RSS; observed average CPU was
  roughly 4.2 cores and peaks reached the eight-logical-core range.
- whisper.cpp `small-q5_1`: about 529 MiB peak RSS, materially lighter but much
  slower here.
- Kokoro: 1.30 GiB peak on the short case, rising to 2.03 GiB on the long
  sustained case.
- Separate resident Whisper and Kokoro workers therefore need roughly 3 GiB at
  observed peaks, before the Rust daemon and OS. This is acceptable on the
  measured 15.35 GiB machine but should be monitored and admission-controlled.
- Artifact storage is approximately 464 MiB for faster-whisper `small` and
  313 MiB for Kokoro plus a 0.5 MiB voice. Caches must stay external to Git.

## Cold Start

| Component | Normal measured load | Degraded/spike load | Consequence |
|---|---:|---:|---|
| faster-whisper small/int8 | 1.03 s | 2.37 s in offline spike | Do not spawn per utterance. |
| Kokoro-82M | 1.94–2.15 s early | 5.10–5.19 s sustained | Keep model and voice resident. |
| whisper.cpp small-q5_1 | ~0.16 s model mapping | first process 4.67 s total | Persistent embedding/server would still avoid process setup. |

Warm, supervised local workers are required. Startup belongs at application
activation, and readiness must be explicit before audio is accepted.

## Warm Inference

faster-whisper `small/int8` was the only tested configuration that combined
good-enough translation quality with consistently sub-real-time throughput.
The 1.67 s short and 1.93 s medium medians are utterance-completion batch
latencies, not first partials. `base/int8` is dramatically faster, but its
semantic failures make it unsuitable as the default; it can remain a future
low-resource option with a clear quality warning.

## Kokoro Performance

The committed four-thread file represents the later sustained, battery-limited
run:

| Text | Generated audio | Synthesis | TTFA/yield | RTF | Peak RAM |
|---|---:|---:|---:|---:|---:|
| Short | 2.55 s | 2.57 s | 2.57 s | 1.009 | 1.30 GiB |
| Medium (2 sentences) | 12.40 s | 11.32 s | 11.32 s | 0.913 | 1.65 GiB |
| Technical | 9.78 s | 8.83 s | 8.83 s | 0.903 | 1.63 GiB |
| Long (about 120 words) | 65.98 s | 64.28 s | 23.76 s | 0.974 | 2.03 GiB |

Earlier in the same session, Kokoro measured 0.46–0.50 RTF with a 1.26 s short
case and 31.41 s long case; the retained earlier eight-thread run similarly
measured median RTF 0.472. Eight threads roughly doubled CPU consumption but
did not materially beat the early four-thread RTF, so four physical-core
threads are preferred.

Round-tripping generated WAVs through faster-whisper `small` reproduced the
short, medium and long texts essentially exactly. The technical case preserved
meaning and normalized “16 kilohertz”/“250 millisecond” to “16 kHz”/“250 ms”;
sequence similarity was 0.908. This is a useful intelligibility proxy, **not a
human listening test**. No clipping was observed in the waveform statistics.
Subjective naturalness, pauses and voice preference still require headphones
and a human reviewer.

## Offline PT-BR → EN Spike

The spike used FLEURS `short_05` (5.22 s):

```text
PT-BR: A polícia disse que o corpo parecia estar ali há cerca de um dia.
EN:    The police said that the body seemed to be there for about a day.
```

It produced a valid 3.85 s, mono, 24 kHz PCM-S16 WAV. Under the late-session
power state, measured times were:

- faster-whisper load: 2.37 s;
- direct translation: 3.47 s;
- Kokoro load: 5.10 s;
- synthesis: 3.67 s;
- warm-stage total (translation + synthesis): 7.14 s;
- cold total: 14.61 s.

Result: **PASS for local functionality; FAIL for a demonstrated 1.5–3 s
sustained live budget on this run.** Generated text/audio remains gitignored.

## License Review

This table records upstream declarations, not legal advice.

| Component | Code license | Model license | Voice license | Commercial use | Redistribution | Notes |
|---|---|---|---|---|---|---|
| OpenAI Whisper | MIT | MIT | N/A | Yes | Yes, retain notice | Official README states code and model weights are MIT ([source](https://github.com/openai/whisper/blob/main/README.md#license)). |
| whisper.cpp | MIT | Uses Whisper MIT weights | N/A | Yes | Yes, retain notice | Official repository/license ([source](https://github.com/ggml-org/whisper.cpp/blob/master/LICENSE)). |
| faster-whisper | MIT | Converted Whisper MIT weights | N/A | Yes | Yes, retain notices/model provenance | Official MIT license ([source](https://github.com/SYSTRAN/faster-whisper/blob/master/LICENSE)). |
| CTranslate2 | MIT | N/A runtime | N/A | Yes | Yes, retain notice | Official repository/license ([source](https://github.com/OpenNMT/CTranslate2/blob/master/LICENSE)). |
| Kokoro Python 0.9.4 | Apache-2.0 | N/A | N/A | Yes | Yes, Apache notice/terms | Official package metadata and license ([source](https://github.com/hexgrad/kokoro/blob/main/LICENSE)). |
| Kokoro-82M | N/A | Apache-2.0 repository/model-card metadata | Repository-level Apache-2.0 metadata; no separate `af_heart` license found | Model: yes | Model: yes under Apache terms | Model card says Apache-licensed weights ([source](https://huggingface.co/hexgrad/Kokoro-82M)). |
| `af_heart.pt` | N/A | N/A | **LICENSE REVIEW REQUIRED** before bundling/redistributing | Local use appears within repository grant; obtain legal review for product distribution | **REVIEW REQUIRED** | The voice file lives in the Apache-tagged model repo and is documented in its official voice list, but no per-voice license/provenance grant was found ([source](https://huggingface.co/hexgrad/Kokoro-82M/blob/main/VOICES.md)). |
| FLEURS benchmark corpus | N/A | N/A | CC-BY-4.0 audio/dataset | Yes with attribution | Yes with attribution | Benchmark input only; not shipped ([source](https://huggingface.co/datasets/google/fleurs)). |

The tested inference stack has no NC component. The only unresolved packaging
question is whether OSB should redistribute a Kokoro voice asset or make the
user-side worker fetch it with displayed attribution/terms. Resolve that before
making any voice a bundled default.

## Operational Complexity

### whisper.cpp

Strengths:

- Single native C/C++ runtime with small q5 files and low RSS.
- Easy CPU-only build and plausible direct embedding in a Rust-owned worker.
- Fewer Python packaging/G2P dependencies.

Weaknesses:

- `small-q5_1` was too slow under this machine's sustained battery state.
- The CLI-per-utterance harness is not a production architecture.
- `base-q5_1` is fast enough but its translation quality was inadequate.

### faster-whisper / CTranslate2

Strengths:

- Best measured speed/quality trade-off with `small/int8`.
- True warm reuse in one process; direct CPU INT8 support.
- Decoding API supports timestamps/segments and future controlled batching.

Weaknesses:

- Python environment, native wheel, model conversion/cache and FFmpeg/PyAV
  supply-chain complexity.
- About 870 MiB peak RSS for `small`, higher than whisper.cpp.
- Must be isolated and supervised; it must never run in a Rust audio callback.

## Comparison

| Criterion | whisper.cpp small-q5_1 | faster-whisper small/int8 |
|---|---|---|
| Translation quality | Similar Whisper capacity; good on many cases, residual noun errors | Similar/best observed; retained more meaning in several examples |
| CPU throughput | Highly sensitive; later full pass near RTF 1 | RTF 0.239 median, long RTF 0.110 |
| Peak RSS | ~529 MiB | ~870 MiB |
| Artifact size | ~181 MiB | ~461 MiB |
| Deployment | Native build/binary, simpler | Python venv + native CTranslate2/PyAV |
| Rust integration | Direct FFI or isolated native worker | Isolated Python worker strongly preferred |
| Failure isolation | Worker recommended | Worker mandatory |
| Future streaming | Upstream stream example, but task needs OSB segmentation | Segment generator; still not true simultaneous Whisper translation |
| License | MIT code + MIT weights | MIT code/runtime + MIT-derived weights |

The speed gap is large enough to outweigh faster-whisper's packaging and memory
cost for the first MVP. This is not a permanent engine lock-in: whisper.cpp
should remain a provider candidate for simpler native deployment or lower-RAM
systems.

## Recommendation

1. **Default ASR/translation candidate:** faster-whisper 1.2.1 on CTranslate2
   4.8.2.
2. **Model:** multilingual `small`, revision pinned, CPU `int8`.
3. **CPU configuration:** four inference threads and one worker. Do not use all
   eight logical threads by default.
4. **TTS candidate:** Kokoro-82M with one English voice, but do not bundle the
   voice until the per-voice redistribution review is resolved.
5. **Runtime:** two persistent, crash-isolated local inference workers (or one
   supervised worker initially if memory requires), started once and kept warm.
6. **IPC:** for the next narrow spike, use a versioned length-delimited protocol
   over a Unix domain socket. Send paths/shared bounded audio descriptors rather
   than JSON/base64 audio. Do not implement gRPC yet; its dependency surface is
   not justified by this benchmark.

The production choice should be encoded in an ADR only after the AC-powered
repeat confirms the operating envelope. The provider boundary must preserve a
future whisper.cpp implementation.

## Expected Live Pipeline Impact

For a short utterance in the earlier regime, faster-whisper `small` (~1.67 s)
plus Kokoro short synthesis (~1.26 s) totals about 2.93 s before VAD endpoint,
queueing, IPC and PipeWire. That is borderline, not a comfortable 1.5–3 s
budget. Longer sentence-level synthesis exceeded the budget, and sustained
battery results were worse.

The likely path to useful conversation is not a faster batch wrapper alone:

- keep both models warm;
- segment conservatively at clause/sentence boundaries;
- begin TTS on stable translated clauses rather than the complete monologue;
- overlap capture/next ASR with current TTS/playback;
- bound every queue and expose endpoint-to-first-audio, underrun and drop
  metrics;
- measure first audible chunk rather than full-WAV completion.

No evidence here supports a sub-500 ms claim. A robust 1.5–3 s first-audio
target remains **uncertain but worth testing** for short clauses on AC power.

## Risks

- Power/thermal state changed during the run; absolute cross-run timing is not
  controlled enough for a release SLA.
- FLEURS is read speech, includes duplicate text across speakers, and lacks
  natural conversation, noise, crosstalk and 2–5 s clips.
- `small` still mistranslated key nouns and technical concepts.
- Kokoro's Python path consumed up to ~2 GiB RSS and did not yield short inputs
  incrementally.
- Two warm workers increase steady memory and packaging complexity.
- A separate `af_heart` redistribution grant was not found.
- Round-trip ASR is not a replacement for human TTS listening.
- Current tool versions are pinned for reproduction but will need deliberate
  security/compatibility updates.

## Open Questions

1. What are the same four-thread measurements on AC power with the governor and
   temperatures recorded before each randomized run?
2. Can sentence/clauses be fed separately to Kokoro to reach useful first audio
   without audible discontinuities?
3. Does `small` meet quality needs on spontaneous Brazilian Portuguese,
   accents, names, numbers and meeting noise?
4. Can a lower-memory Kokoro runtime (for example an officially compatible
   ONNX path) retain license clarity and quality? Not part of this milestone.
5. Will OSB fetch `af_heart` at setup time or redistribute it? Legal review is
   required before choosing the latter.
6. Should whisper.cpp remain the low-memory fallback after a controlled native
   persistent-server comparison?

## Next Milestone Recommendation

Run a **narrow M2.1 controlled inference validation**, not PipeWire integration:

1. repeat faster-whisper `small/int8` and Kokoro on AC power with randomized
   order, three repetitions and temperature/power metadata;
2. add a small consented spontaneous PT-BR conversational corpus while keeping
   recordings private and gitignored;
3. test clause/sentence Kokoro chunking and measure endpoint-to-first-audio plus
   audible joins;
4. have a human listen to the committed test texts' generated WAVs;
5. resolve `af_heart` redistribution and record the engine decision in an ADR;
6. only then build the smallest persistent worker/Unix-socket spike.

Do **not** connect inference to PipeWire, add VAD, or redesign the Rust provider
system as part of this follow-up without separate authorization.

## Reproduction and Raw Results

Commands and cache policy are documented in
[`tools/benchmark/README.md`](../tools/benchmark/README.md). Structured outputs:

- [`benchmark-results.csv`](../tools/benchmark/results/benchmark-results.csv)
- [`benchmark-summary.json`](../tools/benchmark/results/benchmark-summary.json)
- per-engine JSONL files in [`tools/benchmark/results/`](../tools/benchmark/results/)
- [`offline-spike.json`](../tools/benchmark/results/offline-spike.json)
- [`kokoro-intelligibility.json`](../tools/benchmark/results/kokoro-intelligibility.json)
- [`python-freeze.txt`](../tools/benchmark/results/python-freeze.txt)

Model binaries, FLEURS audio, caches, virtual environments and generated WAVs
are intentionally excluded from Git.
