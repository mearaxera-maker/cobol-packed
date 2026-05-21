use cobol_packed::{
    from_packed_scalar, simd_matches_scalar, to_packed_into, PackedConfig, SignMode,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use rust_decimal::Decimal;

fn bench_decode(c: &mut Criterion) {
    let cfg = PackedConfig::new(18, 2, true).unwrap();
    let bytes = vec![0x01, 0x23, 0x45, 0x67, 0x89, 0x01, 0x23, 0x45, 0x67, 0x8C];
    let mut group = c.benchmark_group("packed_decode");
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("single_field_scalar", |b| {
        b.iter(|| {
            black_box(
                from_packed_scalar(black_box(&bytes), black_box(&cfg), SignMode::Nopfd).unwrap(),
            )
        })
    });
    group.finish();
}

fn bench_batch_decode(c: &mut Criterion) {
    let cfg = PackedConfig::new(18, 2, true).unwrap();
    let field = [0x01, 0x23, 0x45, 0x67, 0x89, 0x01, 0x23, 0x45, 0x67, 0x8C];
    let records: Vec<u8> = field.repeat(10_000);
    let mut group = c.benchmark_group("packed_batch_decode");
    group.throughput(Throughput::Bytes(records.len() as u64));
    group.bench_function("ten_thousand_18_digit_fields", |b| {
        b.iter(|| {
            let mut valid = 0usize;
            for chunk in black_box(&records).chunks_exact(field.len()) {
                let _ = from_packed_scalar(chunk, black_box(&cfg), SignMode::Nopfd).unwrap();
                valid += 1;
            }
            black_box(valid)
        })
    });
    group.finish();
}

fn bench_encode_into(c: &mut Criterion) {
    let cfg = PackedConfig::new(18, 2, true).unwrap();
    let value = Decimal::from_i128_with_scale(123456789012345678, 2);
    let mut out = [0u8; 10];
    c.bench_function("encode_into", |b| {
        b.iter(|| {
            to_packed_into(black_box(&value), black_box(&cfg), black_box(&mut out)).unwrap();
            black_box(out)
        })
    });
}

fn bench_simd_validation(c: &mut Criterion) {
    let bytes = vec![0x12; 1 << 20];
    let mut group = c.benchmark_group("simd_validation");
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("simd_matches_scalar", |b| {
        b.iter(|| black_box(simd_matches_scalar(black_box(&bytes))))
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_decode,
    bench_batch_decode,
    bench_encode_into,
    bench_simd_validation
);
criterion_main!(benches);
