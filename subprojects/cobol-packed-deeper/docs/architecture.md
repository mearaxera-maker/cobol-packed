# Architecture: cobol_packed

## Design Philosophy

`cobol_packed` is built on a single principle: **one source of truth**.

The scalar decoder is the reference model. Every other path — SIMD, const-generic, lossless — must produce results that are identical to or a strict superset of the scalar path's output. No alternative decoder can override or reinterpret scalar semantics.

---

## Layer Diagram

```
╔═══════════════════════════════════════════════════════╗
║                   Public API Surface                  ║
║  Packed<D,S,SIGNED>   PackedConfig   PackedPolicy     ║
╚════════════════╤══════════════════════════════════════╝
                 │
       ┌─────────▼──────────┐
       │   Policy Engine    │
       │  SignMode          │   Decides which sign nibbles are
       │  ZeroSignPolicy    │   acceptable and how zero is encoded.
       │  PackedPolicy      │
       └─────────┬──────────┘
                 │
    ┌────────────▼────────────────────────┐
    │      Scalar Reference Decoder       │  ← SOURCE OF TRUTH
    │  from_packed_scalar()               │
    │  from_packed_lossless_scalar()      │
    └────────────┬────────────────────────┘
                 │            ▲
                 │            │  validated against
                 │    ┌───────┴──────────────────┐
                 │    │   SIMD Validation Layer   │
                 │    │  expand_nibbles_avx2()    │
                 │    │  expand_nibbles_sse2()    │
                 │    │  (debug_assert! parity)   │
                 │    └───────────────────────────┘
                 │
    ┌────────────▼────────────────────────┐
    │         Encode Paths                │
    │  to_packed()         canonical      │
    │  to_packed_lossless()  forensic     │
    │  to_packed_with_sign() explicit     │
    │  to_packed_into()    stack-only     │
    └─────────────────────────────────────┘
```

---

## Key Invariants

### 1. Scalar Truth Law

```
∀ valid bytes b:
  from_packed(b) == from_packed_scalar(b)
```

SIMD paths expand nibbles as a performance optimisation. They are
cross-checked against the scalar path in debug builds via `debug_assert!`.
A maintenance change that causes SIMD divergence will be caught immediately.

### 2. Overflow Law

```
max representable = 10^total_digits - 1
```

The boundary is `10^D - 1`, not `10^D`. This is enforced before encoding
and checked against the precomputed `POW10` table, not a runtime multiply
that could overflow.

### 3. Lossless Law

```
∀ valid bytes b (in accepted sign domain):
  to_packed_lossless(from_packed_lossless(b)) == b
```

The lossless codec preserves the original sign nibble. It does **not**
normalise negative zero (`000D`) to positive zero (`000C`), and it also
preserves the positive unsigned sign family (`0xA`, `0xC`, `0xE`, `0xF`)
when the input policy accepts it. This is required for forensic migration
tooling where byte-for-byte identity against the mainframe source must be
verifiable.

### 4. Policy Separation Law

The canonical encode path and the lossless encode path must never share a
zero-normalisation shortcut. They are separate code paths; conflating them
is a correctness bug, not a performance trade-off.

### 5. No-Panic Law

```
∀ inputs (bytes, config, value):
  no call to any public API panics
```

All failure cases return `Err(PackedError::...)`. `Decimal::MIN` is
explicitly rejected before any arithmetic that would overflow `i128`.

---

## Module Layout

```
src/
  lib.rs       — public API, codec logic, tests
  simd.rs      — SIMD nibble expansion (SSE2 + AVX2), validated against scalar

benches/
  packed_bench.rs  — Criterion benchmarks: decode, encode-into, SIMD validation

examples/
  basic.rs         — canonical encode/decode walkthrough
  forensic_zero.rs — lossless round-trip and negative-zero handling

docs/
  formal_spec.md   — mathematical specification of the codec laws
  architecture.md  — this file

fuzz/
  fuzz_targets/fuzz_decode.rs  — libFuzzer harness targeting from_packed

kani/
  proofs.rs  — Kani model-checking proof harnesses for the core invariants
```

---

## Data Flow: Decode

```
raw bytes  ──►  length check  ──►  nibble extraction  ──►  digit validation
                                         │
                               SIMD path (if avx2/sse2)
                               validated against scalar in debug
                                         │
                               sign nibble extraction  ──►  policy check
                                         │
                               magnitude assembly  ──►  overflow check
                                         │
                               scale application  ──►  Decimal output
```

## Data Flow: Encode

```
Decimal  ──►  sign check  ──►  magnitude extraction (no abs())
                                    │
                               scale normalisation  ──►  overflow check
                                    │
                               digit packing  ──►  sign nibble appended
                                    │
                               canonical: zero normalised to 0xC/0xF
                               lossless:  zero sign nibble preserved
                                    │
                               Vec<u8> or &mut [u8] (stack-only path)
```
