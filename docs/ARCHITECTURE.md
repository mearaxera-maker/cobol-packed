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

All standard failure cases return `Err(PackedError::...)`; strict precision
checks return `Err(StrictPackedError::...)`. `Decimal::MIN` is explicitly
rejected before any arithmetic that would overflow `i128`, and internal packing
helpers must validate caller-provided buffer length instead of asserting.

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
  ARCHITECTURE.md  — this file

fuzz/
  fuzz_targets/fuzz_decode.rs  — libFuzzer harness targeting from_packed

kani/
  proofs.rs  — Kani model-checking proof harnesses for the core invariants
```

---

## CLI Schema v2 Record Engine

The CLI has two schema layers:

- Raw user declarations: deserialized v1/v2 schema files, preserving operator
  intent such as declared offsets, sequential layout mode, SYNC flags,
  REDEFINES variants, and OCCURS DEPENDING ON counters.
- Planned schema: a validated canonical layout with absolute byte ranges,
  codec dispatch, coverage ranges, and semantic-hash input.

Schema v1 lowers to a declared v2-style internal plan but remains COMP-3-only
and source-compatible. Schema v2 uses `layout_mode` to prevent ambiguous mixes
of absolute offsets and computed offsets:

- `declared`: every top-level field/group has an explicit absolute offset.
- `sequential`: offsets are omitted and computed; SYNC may insert synthetic
  `sync-slack` ranges.

The v2 codec dispatcher supports packed decimal through the public library
core, plus CLI-internal codecs for zoned decimal, binary/native COMP,
IBM hexadecimal floats, alphanumeric EBCDIC/ASCII text, and raw filler bytes.
Binary sign policy is represented by signedness and endian only; sign-nibble
policy is limited to decimal encodings.

For forensic verification, zoned decimals and IBM hexadecimal floats carry a
byte-preserving decoded representation internally. Their operator-facing value
is still `Decimal` or `f64`, but verification re-emits the original validated
field bytes instead of treating decode success as sufficient evidence.

REDEFINES is implemented as safe immutable window reinterpretation: each
variant decodes from the same `&[u8]` byte range. Coverage counts the base range
once and variant overlaps are not treated as normal layout overlaps. No unsafe
code, raw pointers, Rust `union`, or transmute are used.

OCCURS DEPENDING ON uses a binary streaming reader that first reads the minimum
header needed for preceding counter fields, validates the count against
`min_occurs..=max_occurs`, computes the actual record length, and then reads
the remaining bytes. Counts are never clamped by default. Generated Rust views
use the same rule: top-level OCCURS groups expose `Vec<Element>` accessors and
reject scaled or nonnumeric counter fields at schema-planning time.

---

The fuzz suite also includes encode/decode roundtrip and schema-aware
batch-layout harnesses. The SIMD fuzz smoke build enables the crate `simd`
feature so parity code is compiled under fuzzing. Kani coverage is bounded:
small fields are checked exhaustively where feasible, and maximum-width
18-digit/10-byte decode has a representative no-panic harness.

Deep verification lives in `.github/workflows/deep-verification.yml`. It runs
time-boxed cargo-fuzz campaigns for scalar, SIMD, roundtrip, and schema-aware
targets, plus the Kani proof harness matrix. Local Windows/MSVC fuzzing may need
the MSVC AddressSanitizer runtime; if that runtime is unavailable, use the Linux
workflow as the authoritative cargo-fuzz campaign runner.

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

Strict encode uses the same pipeline as canonical/lossless encode, but the
scale-normalisation stage rejects non-zero fractional digits that would be
discarded. Legacy encode keeps truncation toward zero for compatibility.

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
