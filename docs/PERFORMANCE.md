# Performance

This document describes performance characteristics, measurement methodology, and optimization targets for OpenSpeechBridge.

## Performance Goals

### Latency Targets

| Metric | Target | Acceptable | Notes |
|--------|--------|------------|-------|
| Capture latency | < 10ms | < 20ms | Mic to buffer |
| Buffer latency | < 20ms | < 50ms | Buffer fill level |
| STT latency | < 500ms | < 1000ms | Audio to text |
| Translation latency | < 100ms | < 300ms | Text to text |
| TTS latency | < 300ms | < 500ms | Text to audio |
| End-to-end | < 1000ms | < 2000ms | Mic to virtual mic |
| Time-to-first-audio | < 500ms | < 1000ms | Start speaking to hearing |

### Throughput Targets

| Metric | Target |
|--------|--------|
| Audio processing | Real-time (1x) minimum |
| CPU usage (idle) | < 5% |
| CPU usage (active) | < 50% single core |
| Memory usage | < 2GB |

### Quality Targets

| Metric | Target |
|--------|--------|
| Dropped frames | < 0.1% |
| Underruns | < 1 per minute |
| Overruns | < 1 per minute |

## Measurement Methodology

### Latency Measurement

```rust
// Capture latency
let capture_start = Instant::now();
// ... capture callback runs ...
let capture_latency = capture_start.elapsed();

// Processing latency
let process_start = Instant::now();
// ... engine processes ...
let process_latency = process_start.elapsed();
```

### End-to-End Latency

Measure with a loopback test:
1. Generate a click/tone
2. Play through speakers
3. Capture with mic
4. Process through pipeline
5. Detect output in virtual mic
6. Calculate total time

### Reporting

Always document:
- Hardware (CPU, RAM, GPU)
- Operating system and version
- Kernel version
- PipeWire version
- Model name and size
- Model precision (fp32, fp16, int8)
- Audio format (sample rate, channels)
- Test methodology

## Current Measurements

> **Note**: No measurements available yet. This section will be updated as the project matures.

### Placeholder

| Test | Hardware | Latency | Notes |
|------|----------|---------|-------|
| TBD | TBD | TBD | TBD |

## Optimization Strategies

### Audio Path

1. **Bounded buffers**: Prevent unbounded memory growth
2. **Lock-free queues**: Avoid blocking in audio callbacks
3. **Preallocated buffers**: No allocations in hot path
4. **SIMD**: Use vectorized operations for conversion

### Engine Path

1. **Quantization**: Use int8/fp16 models when possible
2. **Batching**: Process multiple chunks together
3. **Streaming**: Start output before input complete
4. **GPU offload**: Use CUDA/Metal when available

### Memory

1. **Buffer pooling**: Reuse audio buffers
2. **Lazy loading**: Load models on demand
3. **Model sharing**: Share weights between instances

## Benchmarks

Benchmarks are in the `benchmarks/` directory:

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench --bench buffer_bench
```

### Benchmark Structure

```rust
// benchmarks/buffer_bench.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn ring_buffer_benchmark(c: &mut Criterion) {
    c.bench_function("ring_buffer_push_pop", |b| {
        let (mut prod, mut cons) = AudioRingBuffer::new(4096, 2);
        let input = vec![0.5f32; 512];
        let mut output = vec![0.0f32; 512];
        
        b.iter(|| {
            prod.push(&input);
            cons.pop(&mut output);
        });
    });
}

criterion_group!(benches, ring_buffer_benchmark);
criterion_main!(benches);
```

## Profiling

### CPU Profiling

```bash
# Linux with perf
perf record -g ./target/release/openspeechbridge run
perf report

# With flamegraph
cargo install flamegraph
cargo flamegraph --bin openspeechbridge -- run
```

### Memory Profiling

```bash
# With valgrind
valgrind --tool=massif ./target/release/openspeechbridge run
ms_print massif.out.*

# With heaptrack
heaptrack ./target/release/openspeechbridge run
heaptrack_gui heaptrack.openspeechbridge.*.gz
```

## Metrics Export

Future: Prometheus metrics endpoint

```
# HELP osb_frames_received Total audio frames received
# TYPE osb_frames_received counter
osb_frames_received 1234567

# HELP osb_capture_latency_seconds Capture latency histogram
# TYPE osb_capture_latency_seconds histogram
osb_capture_latency_seconds_bucket{le="0.001"} 100
osb_capture_latency_seconds_bucket{le="0.005"} 450
osb_capture_latency_seconds_bucket{le="0.01"} 500
```

## Contributing Performance Improvements

1. **Measure first**: Don't optimize without data
2. **Document methodology**: How did you measure?
3. **Include hardware specs**: Results vary by hardware
4. **Run benchmarks**: Ensure no regressions
5. **Update this document**: Add new measurements
