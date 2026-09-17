use criterion::{black_box, criterion_group, criterion_main, Criterion};
use cobol_packed::{from_packed_scalar, simd_matches_scalar, to_packed_into, PackedConfig, SignMode};
use rust_decimal::Decimal;

fn bench_decode(c: &mut Criterion) {
let cfg = PackedConfig::new(18, 2, true).unwrap();
let bytes = vec![0x12, 0x34, 0x56, 0x78, 0x90, 0x12, 0x34, 0x56, 0x78, 0x0C];
c.bench_function("decode_scalar", |b| {
b.iter(|| black_box(from_packed_scalar(black_box(&bytes), black_box(&cfg), SignMode::Nopfd).unwrap()))
});
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
c.bench_function("simd_matches_scalar", |b| {
b.iter(|| black_box(simd_matches_scalar(black_box(&bytes))))
});
}

criterion_group!(benches, bench_decode, bench_encode_into, bench_simd_validation);
criterion_main!(benches);
