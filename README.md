# cobol_packed
### Forensic-Grade, SIMD-Accelerated COMP-3 for Rust

![Rust](https://img.shields.io/badge/rust-stable-orange)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)
![Status](https://img.shields.io/badge/status-active-success)
![SIMD](https://img.shields.io/badge/SIMD-AVX2%20%7C%20SSE2-purple)
![Fuzzed](https://img.shields.io/badge/fuzzed-nibble--space-critical)

> A zero-allocation, no-panic packed decimal codec designed for high-frequency fintech systems, forensic migration tooling, and mainframe modernization.

`cobol_packed` treats COMP-3 as a binary contract — not a convenience format.

---

## Why This Exists

Most packed-decimal bugs are not obvious:

- negative-zero normalization
- off-by-one overflow boundaries
- malformed sign nibble acceptance
- SIMD/scalar drift
- invalid even-digit padding
- hidden `.abs()` overflows

This crate was built to eliminate those classes of failures.

---

## Features

- Scalar reference decoder (source of truth)
- Optional AVX2/SSE2 validation layer
- Const-generic codecs (`Packed::<18,2,true>`)
- Stack-only encode APIs
- Lossless forensic round-trips
- Explicit sign policy engine
- Exact overflow law (`10^digits - 1`)
- Exhaustive nibble fuzzing
- Criterion benchmarks
- Kani proof harness

---

## Quick Start

```rust
use cobol_packed::{Packed, SignMode};

type Balance = Packed<15, 2, true>;

fn main() {
    let codec = Balance::new();

    let raw = [
        0x00,
        0x00,
        0x00,
        0x12,
        0x34,
        0x5C,
    ];

    let value = codec.decode(&raw, SignMode::Pfd).unwrap();

    println!("{}", value); // 123.45
}
```

---

## Architecture

```text
Packed Bytes
    ↓
Scalar Reference Decoder  ← SIMD Validation Layer
    ↓
Policy Engine
    ↓
Decimal Representation
    ↓
Canonical | Explicit-Sign | Lossless Encode
```

---

## Red-Team Notes

### Negative Zero

`000D` and `000C` are not treated as identical in forensic mode.

### Overflow Law

The maximum representable value is:

```text
10^digits - 1
```

not:

```text
10^digits
```

### Scalar Truth Law

SIMD never overrides scalar semantics.

The scalar decoder is the authoritative model.

---

## Repository Layout

```text
/src
/benches
/docs
/examples
/fuzz
/kani
```

---

## License

MIT OR Apache-2.0
