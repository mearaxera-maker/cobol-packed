# Draft Migration Guide: v0.6 → v1.0

This draft covers planned breaking changes for a future `cobol_packed` v1.0
and provides a mechanical upgrade path for each one.

---

## Breaking Changes

### 1. `PackedPolicy` is now required for lossless APIs

**Before (v0.6):**
```rust
let decoded = from_packed_lossless(&bytes, &cfg, SignMode::Nopfd)?;
let repacked = to_packed_lossless(&decoded, &cfg)?;
```

**After (v1.0):**
```rust
use cobol_packed::{PackedPolicy, SignMode};

let policy = PackedPolicy::lossless(SignMode::Nopfd);
let decoded = from_packed_lossless_with_policy(&bytes, &cfg, policy)?;
let repacked = to_packed_lossless_with_policy(&decoded, &cfg, policy)?;

// Or use the shorthand helpers which infer the canonical policy:
let decoded = from_packed_lossless(&bytes, &cfg, SignMode::Nopfd)?;
```

The bare `SignMode` signatures are retained as convenience wrappers
over `PackedPolicy::lossless(sign_mode)`. No code changes are required
unless you need explicit `ZeroSignPolicy` control.

### 2. `Packed<D, S, SIGNED>::len()` is deprecated in favor of `LEN`

**Before (v0.6):**
```rust
let n = Packed::<18, 2, true>::len();
```

**After (v1.0):**
```rust
let n = Packed::<18, 2, true>::LEN; // associated constant
```

The deprecated `len()` method is still present for compatibility, but new code
should use `LEN`. `LEN` is a `const`, so it can be used in array sizes:
```rust
let mut buf = [0u8; Packed::<18, 2, true>::LEN];
```

### 3. `simd_matches_scalar` is available when the `simd` feature is enabled

**Before (v0.6):**
```rust
use cobol_packed::simd_matches_scalar;
```

**After (v1.0):**
```rust
use cobol_packed::simd_matches_scalar; // available when feature `simd` is enabled
```

### 4. `PackedConfig::new` scale validation is explicit

The accepted range is `0..=total_digits`, and `total_digits` is bounded to
`1..=18`. Configs with larger scale values return
`Err(PackedError::ScaleExceedsTotalDigits { .. })`. The older
`ScaleTooLargeForDecimal` variant is retained only for source compatibility.

---

## New in v1.0

### MSRV update

The minimum supported Rust version is now 1.74. The library now ships a
production CLI binary, and the CLI dependency stack requires the newer MSRV.

### `PackedConfig` convenience methods

```rust
let cfg = PackedConfig::signed(12, 2)?;
println!("max: {}", cfg.max_value());   // 9999999999.99
println!("min: {}", cfg.min_value());   // -9999999999.99 (signed only)
println!("len: {}", cfg.byte_len());    // 7
```

### Stack-only encode into `[u8; N]`

```rust
let mut buf = [0u8; Packed::<6, 2, true>::LEN];
Packed::<6, 2, true>::encode_into(&value, &mut buf)?;
// No heap allocation.
```

### `NibbleIter` — zero-allocation nibble streaming

```rust
use cobol_packed::nibble_iter;

for nibble in nibble_iter(&raw_bytes) {
    process(nibble);
}
```

### `to_packed_into` — runtime stack-only encode

```rust
let mut buf = [0u8; 10];
to_packed_into(&value, &cfg, &mut buf)?;
```

### Strict precision-safe encode helpers

Legacy `to_packed*` helpers keep their existing truncating behavior: if a value
has more fractional digits than the configured packed scale, non-zero low-order
fractional digits are truncated toward zero. New strict helpers reject that
case:

```rust
use cobol_packed::{to_packed_strict, StrictPackedError};

match to_packed_strict(&value, &cfg) {
    Ok(bytes) => write_record(bytes),
    Err(StrictPackedError::PrecisionLoss { .. }) => reject_record(value),
    Err(StrictPackedError::Codec(err)) => return Err(err.into()),
}
```

Strict variants are available for owned-buffer, caller-provided buffer,
explicit-sign, policy-based, const-generic, and lossless encode paths.

### Unsigned PFD sign normalization

PFD decode accepts unsigned positive `0xC` signs because they are common in
mainframe files. Canonical unsigned encode emits `0xF`. Use lossless mode when
byte-for-byte preservation of unsigned `0xC` signs is required; canonical mode
is appropriate when sign normalization is part of the migration contract.

---

## Unchanged APIs (safe to keep as-is)

- `from_packed(&bytes, &cfg, sign_mode)` — unchanged
- `to_packed(&value, &cfg)` — unchanged
- `from_packed_scalar` — unchanged
- `stream_nibbles` — unchanged
- `PackedError` variants (additions only; no removals)
- `SignMode::{Pfd, Nopfd}` — unchanged
