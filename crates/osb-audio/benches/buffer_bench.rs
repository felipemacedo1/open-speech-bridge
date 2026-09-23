//! Benchmarks for audio buffer operations.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use osb_audio::buffer::AudioRingBuffer;

fn ring_buffer_push_pop(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer");
    
    // Typical audio: 48kHz stereo, 10ms chunks = 480 frames = 960 samples
    let chunk_size = 960;
    group.throughput(Throughput::Elements(chunk_size as u64));
    
    group.bench_function("push_pop_960_samples", |b| {
        let (mut producer, mut consumer) = AudioRingBuffer::new(4096, 2);
        let input = vec![0.5f32; chunk_size];
        let mut output = vec![0.0f32; chunk_size];
        
        b.iter(|| {
            producer.push(black_box(&input));
            consumer.pop(black_box(&mut output));
        });
    });
    
    group.bench_function("push_only_960_samples", |b| {
        let (mut producer, mut consumer) = AudioRingBuffer::new(8192, 2);
        let input = vec![0.5f32; chunk_size];
        
        b.iter(|| {
            producer.push(black_box(&input));
            // Drain occasionally to prevent overflow
            if producer.occupancy().fill_ratio > 0.8 {
                let mut drain = vec![0.0f32; 4096];
                consumer.pop(&mut drain);
            }
        });
    });
    
    group.finish();
}

fn ring_buffer_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer_sizes");
    
    for size in [256, 512, 1024, 2048, 4096].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        
        group.bench_function(format!("push_pop_{}_samples", size), |b| {
            let (mut producer, mut consumer) = AudioRingBuffer::new(8192, 2);
            let input = vec![0.5f32; *size];
            let mut output = vec![0.0f32; *size];
            
            b.iter(|| {
                producer.push(black_box(&input));
                consumer.pop(black_box(&mut output));
            });
        });
    }
    
    group.finish();
}

criterion_group!(benches, ring_buffer_push_pop, ring_buffer_sizes);
criterion_main!(benches);
