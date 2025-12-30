//! Benchmarks for interleaved RaptorQ encoder/decoder

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

// Import from the crate itself
extern crate raptorq_c_bindings;
use raptorq_c_bindings::interleave::{InterleavedDecoder, InterleavedEncoder};

fn benchmark_encoder_add_packet(c: &mut Criterion) {
    let depth = 4;
    let k = 8;
    let symbol_size = 1200;
    let repair_symbols = 4;

    let mut group = c.benchmark_group("encoder_add_packet");
    group.throughput(Throughput::Bytes(symbol_size as u64));

    group.bench_function("depth_4_k_8", |b| {
        let mut encoder = InterleavedEncoder::new(depth, k, symbol_size, repair_symbols).unwrap();
        let data = vec![0u8; symbol_size as usize];

        b.iter(|| {
            black_box(encoder.add_packet(&data).unwrap());
        });
    });

    group.finish();
}

fn benchmark_encoder_throughput(c: &mut Criterion) {
    let depth = 4;
    let k = 8;
    let symbol_size = 1200;
    let repair_symbols = 4;
    let packets_per_iter = 1000;

    let mut group = c.benchmark_group("encoder_throughput");
    group.throughput(Throughput::Elements(packets_per_iter as u64));

    group.bench_function("1000_packets", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;

            for _ in 0..iters {
                let mut encoder =
                    InterleavedEncoder::new(depth, k, symbol_size, repair_symbols).unwrap();
                let data = vec![0u8; symbol_size as usize];

                let start = std::time::Instant::now();
                for _ in 0..packets_per_iter {
                    black_box(encoder.add_packet(&data).unwrap());
                }
                total += start.elapsed();
            }

            total
        });
    });

    group.finish();
}

fn benchmark_generate_repair(c: &mut Criterion) {
    let depth = 4;
    let k = 8;
    let symbol_size = 1200;
    let repair_symbols = 4;

    let mut group = c.benchmark_group("generate_repair");
    group.throughput(Throughput::Bytes(
        (repair_symbols * symbol_size as u32) as u64,
    ));

    group.bench_function("depth_4_k_8_repair_4", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;

            for _ in 0..iters {
                let mut encoder =
                    InterleavedEncoder::new(depth, k, symbol_size, repair_symbols).unwrap();
                let data = vec![0u8; symbol_size as usize];

                // Fill all blocks
                for _ in 0..(depth * k) {
                    encoder.add_packet(&data).unwrap();
                }

                // Benchmark generating repair for all blocks
                let start = std::time::Instant::now();
                for block_idx in 0..depth {
                    black_box(encoder.generate_repair(block_idx as usize).unwrap());
                }
                total += start.elapsed();
            }

            total
        });
    });

    group.finish();
}

fn benchmark_decoder_add_packet(c: &mut Criterion) {
    let depth = 4;
    let k = 8;
    let symbol_size = 1200;
    let repair_symbols = 4;

    let mut group = c.benchmark_group("decoder_add_packet");
    group.throughput(Throughput::Bytes(symbol_size as u64));

    group.bench_function("depth_4_k_8", |b| {
        // Create encoder and get OTI
        let encoder = InterleavedEncoder::new(depth, k, symbol_size, repair_symbols).unwrap();
        let oti = encoder.get_oti();

        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;

            for _ in 0..iters {
                let mut decoder = InterleavedDecoder::new(&oti, depth).unwrap();

                // Create test packets
                let mut packets = Vec::new();
                for block_id in 0..depth {
                    for symbol_id in 0..k {
                        let mut packet = vec![0u8; symbol_size as usize];
                        packet.extend_from_slice(&(block_id as u32).to_be_bytes());
                        packet.extend_from_slice(&(symbol_id as u32).to_be_bytes());
                        packets.push(packet);
                    }
                }

                let start = std::time::Instant::now();
                for packet in &packets {
                    black_box(decoder.add_packet(packet).ok());
                }
                total += start.elapsed();
            }

            total
        });
    });

    group.finish();
}

fn benchmark_full_roundtrip(c: &mut Criterion) {
    let depth = 4;
    let k = 8;
    let symbol_size = 1200;
    let repair_symbols = 4;

    let mut group = c.benchmark_group("full_roundtrip");
    group.throughput(Throughput::Bytes((depth * k * symbol_size as u32) as u64));

    group.bench_function("depth_4_k_8", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;

            for _ in 0..iters {
                let mut encoder =
                    InterleavedEncoder::new(depth, k, symbol_size, repair_symbols).unwrap();
                let oti = encoder.get_oti();
                let mut decoder = InterleavedDecoder::new(&oti, depth).unwrap();

                // Create source data
                let data = vec![0u8; symbol_size as usize];

                let start = std::time::Instant::now();

                // Encode
                for _ in 0..(depth * k) {
                    encoder.add_packet(&data).unwrap();
                }

                // Get source packets and decode
                for block_idx in 0..depth {
                    let source_packets = encoder.get_source_packets(block_idx as usize).unwrap();
                    let packet_size = 8 + symbol_size as usize;

                    for i in 0..(k as usize) {
                        let pkt_start = i * packet_size;
                        let pkt_end = pkt_start + packet_size;
                        let packet = &source_packets[pkt_start..pkt_end];

                        // Reformat for decoder
                        let mut decoder_packet = Vec::new();
                        decoder_packet.extend_from_slice(&packet[8..]);
                        decoder_packet.extend_from_slice(&packet[0..8]);

                        decoder.add_packet(&decoder_packet).ok();
                    }
                }

                total += start.elapsed();
            }

            total
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_encoder_add_packet,
    benchmark_encoder_throughput,
    benchmark_generate_repair,
    benchmark_decoder_add_packet,
    benchmark_full_roundtrip,
);

criterion_main!(benches);
