# Architecture: How cobol_packed Works

This document explains the internals so you can understand why certain design decisions were made, debug issues confidently, and extend the codec safely.

---

## High-Level Design

```
Packed Decimal Bytes (from COBOL/Legacy)
                |
                v
     Byte-Level Validation
     - Length check
     - Even/odd digit count
     - Sign nibble validation
                |
                v
     Scalar Reference Decoder (TRUTH)
     - Extract nibbles
     - Build i128 value
     - Track sign
                |
                +-- SIMD Validator (if feature="simd")
                |   - Verify SIMD matches scalar
                |
                v
     Policy Engine
     - Normalize sign (if needed)
     - Apply sign preference
     - Track forensic metadata
                |
                v
     Decimal Value
     struct Decimal {
       value: i128,
       scale: u32,
       forensic: Option<Meta>
     }
```

---

## Core Concepts

### 1. Packed Decimal Format (COMP-3)

A packed decimal stores 2 decimal digits per byte:

```
Byte Structure:
High Nibble | Low Nibble
Digit 0     | Digit 1

Example: 123.45
Storage:
  Digit: 1 2 | 3 4 | 5 (sign)
  Byte:  0x12 | 0x34 | 0x5C
         (sign=C for +)
```

Sign nibbles:

- 0x0C or 0x0F = positive preferred
- 0x0D or 0x0B = negative preferred
- 0x0A = unsigned (rare)
- Anything else = forensic anomaly

### 2. Const-Generic Codec

```rust
type Balance = Packed<15, 2, true>;
//                   |   |  |
//                   |   |  +-- Signed?
//                   |   +------ Decimal scale (2 places)
//                   +---------- Total digits (15)
```

Why const-generics?

- Compile-time validation (overflow impossible at compile-time for fixed types)
- Zero runtime overhead
- Type-safe (can't accidentally swap digit counts)

### 3. Nibble Extraction

Packed decimals store digits as 4-bit nibbles. We extract them left-to-right:

```rust
fn extract_nibbles(bytes: &[u8]) -> Vec<u8> {
    let mut nibbles = Vec::new();
    for byte in bytes {
        nibbles.push((byte >> 4) & 0x0F);  // High nibble
        nibbles.push(byte & 0x0F);         // Low nibble
    }
    nibbles
}

// Input:  [0x12, 0x34, 0x5C]
// Output: [0x1, 0x2, 0x3, 0x4, 0x5, 0xC] (last is sign)
```

### 4. i128 Construction

From nibbles, we build an i128 value:

```rust
let mut value: i128 = 0;
for (i, &nibble) in digit_nibbles.iter().enumerate() {
    value = value * 10 + (nibble as i128);
}
// Now scale it: 123450 with scale=2 -> 123.45
let scaled = value / 10^scale;
```

Why i128?

- Handles up to 38 digits (enough for COBOL)
- Signed (can represent negative)
- Exact integer arithmetic (no floating-point precision loss)

### 5. Overflow Boundary

For Packed<N, S, signed>:

```
Maximum value = 10^N - 1

NOT 10^N. This is critical.
```

Example:

```rust
// Packed<5, 0, true> can store 0 to 99,999
// NOT 100,000

fn check_overflow(value: i128, max_digits: u32) -> Result<(), Error> {
    let max = 10_i128.pow(max_digits) - 1;
    if value.abs() > max {
        Err(OverflowError)
    } else {
        Ok(())
    }
}
```

---

## Decode Process (Step-by-Step)

```
Input: raw bytes [0x12, 0x34, 0x5C]
Target: Packed<5, 2, true>

Step 1: Validate Length
        Input: 3 bytes, digits=5 (needs ceil(5/2)=3 bytes) OK

Step 2: Extract Nibbles
        [0x1, 0x2, 0x3, 0x4, 0x5, 0xC]

Step 3: Separate Sign & Digits
        Digits: [0x1, 0x2, 0x3, 0x4, 0x5]
        Sign: 0xC (positive preferred)

Step 4: Validate Sign
        Is 0xC valid? Yes
        Is it forensic? No (0xC is standard)

Step 5: Build i128
        0*10 + 1 = 1
        1*10 + 2 = 12
        12*10 + 3 = 123
        123*10 + 4 = 1234
        1234*10 + 5 = 12345
        
        Is 12345 > 99999? No

Step 6: Apply Sign
        If negative (D/B): 12345 -> -12345
        Otherwise: 12345

Step 7: Apply Scale
        12345 with scale=2 -> 123.45

Result: Decimal { value: 12345, scale: 2, sign: Positive }
```

---

## Encode Process (Reverse)

```
Input: Decimal { value: 12345, scale: 2 }
Target: Packed<5, 2, true>

Step 1: Check Overflow
        12345 <= 99999? Yes

Step 2: Determine Sign Nibble
        Is negative? No -> Use 0xC (preferred positive)

Step 3: Extract Digits from i128
        12345 -> [0x1, 0x2, 0x3, 0x4, 0x5]

Step 4: Pad if Needed
        Digits: 5 (odd), need even -> Pad left with 0x0
        [0x0, 0x1, 0x2, 0x3, 0x4, 0x5]
        
        Now we have 6 nibbles = 3 bytes

Step 5: Pack Nibbles into Bytes
        [0x0, 0x1] -> 0x01
        [0x2, 0x3] -> 0x23
        [0x4, 0x5 + 0xC0] -> 0x5C (sign in low nibble)

Result: [0x01, 0x23, 0x5C]
```

---

## Forensic Mode

Purpose: Preserve exact bytes for audit compliance.

### How It Works

```rust
enum ForensicMetadata {
    StandardSign,           // 0xC/0xF or 0xD/0xB
    NonStandardSign(u8),    // 0x0D instead of 0xC?
    PaddingAnomaly,         // Expected 0x0X, got 0xYX
    DigitMismatch,          // Binary representation doesn't match nibbles
}
```

### Example: Negative Zero

```
Bytes: [0x00, 0x00, 0x0D]
Decoded: value=0, sign=negative, forensic=true

value.canonical() -> "0"              // Normalized for logic
value.explicit_sign() -> "-0"         // Audit trail
value.forensic_bytes() -> [0x00, 0x00, 0x0D]  // Original
value.forensic_metadata() -> NonStandardSign(0x0D)
```

This is critical for compliance. You can:

1. Use normalized values for business logic
2. Log forensic metadata for audits
3. Prove round-trip integrity

---

## SIMD Validation Layer

Purpose: Catch CPU-specific bugs where AVX2 differs from scalar.

### How It Works

```rust
#[cfg(feature = "simd")]
fn validate_simd_matches_scalar(
    raw: &[u8],
    scalar_result: &Decimal,
) -> Result<(), SimdMismatchError> {
    let simd_result = simd_decode(raw)?;
    
    if scalar_result.value != simd_result.value
        || scalar_result.sign != simd_result.sign
    {
        return Err(SimdMismatchError {
            scalar: scalar_result.value,
            simd: simd_result.value,
        });
    }
    
    Ok(())
}
```

Important: The scalar result is authoritative. SIMD is just a validator.

### When SIMD Might Differ

- Overflow handling on edge cases
- Rounding behavior (shouldn't happen, but paranoia wins)
- CPU microcode bugs (rare, but we catch them)

---

## Error Handling

Every operation returns Result<T, DecodeError>:

```rust
#[derive(Debug)]
pub enum DecodeError {
    InvalidLength { expected: usize, got: usize },
    InvalidSign { nibble: u8 },
    Overflow { value: i128, max: i128 },
    PaddingMismatch { expected: u8, got: u8 },
    SimdMismatch { scalar: i128, simd: i128 },
}
```

Philosophy: Fail loudly and early, never silently corrupt data.

---

## Type Safety

```rust
// Compile-time checks:
let val: Packed<5, 0, true> = Packed::decode(&raw, SignMode::Pfd)?;

// What's guaranteed at compile time?
// - Exactly 5 digits
// - Exactly 0 decimal places
// - Signed (can be negative)
// - Overflow boundary: 99,999

// What's checked at runtime?
// - Input bytes match length
// - Sign nibble is valid
// - No CPU drift (SIMD)
```

This combination gives you maximum safety.

---

## Performance Notes

### Decode (Scalar)

1. Validate length (O(1))
2. Extract nibbles (O(n), n = bytes)
3. Build i128 (O(d), d = digits)
4. Apply sign & scale (O(1))

Total: O(n) where n ≈ d/2

### Decode (SIMD)

1. Load bytes into vector register
2. Parallel nibble extraction (4 bytes at once)
3. Parallel addition (Horner's method in parallel)

Total: O(1) amortized per 4 bytes

### Why No Allocation?

- Const-generic types are sized at compile-time
- Decimal struct is [i128, u32, Option<Meta>] = stack-allocated
- No Vec, no String, no Box

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_negative_zero_forensic() {
    let raw = [0x00, 0x00, 0x0D];
    let val = Packed::<5, 0, true>::decode(&raw, SignMode::ForensicExact)?;
    assert_eq!(val.canonical(), "0");
    assert_eq!(val.forensic_bytes(), raw);
}
```

### Property Tests (QuickCheck)

```rust
quickcheck! {
    fn prop_lossless_roundtrip(value: i128) -> bool {
        // Every value should round-trip perfectly
    }
}
```

### Fuzzing

```
targets/fuzz/nibble_space.rs - Generate all possible nibble combinations
```

### Formal Verification (Kani)

```
Proves overflow boundaries are mathematically correct
```

---

## Extension Points

### Adding a New Sign Policy

```rust
pub enum SignMode {
    Pfd,              // Preferred (0xC/0xF or 0xD/0xB)
    ForensicExact,    // Preserve everything
    Explicit,         // Always include sign character
    // Add new policies here
}
```

### Adding COMP-5 (Binary)

```rust
pub struct Packed5<const DIGITS: u32, const SIGNED: bool> {
    // Binary-encoded packed decimals
}
```

### Adding COMP-2 (Floating-Point)

```rust
pub struct Packed2<const SCALE: u32> {
    // IEEE float wrapper
}
```

---

## Security Considerations

### What Can Go Wrong?

1. Integer Overflow - Caught at encode time via overflow law
2. Malformed Bytes - Validated at decode time
3. SIMD Drift - Caught if scalar != SIMD
4. Panic - Impossible (all paths return Result)
5. Unsafe Code - None in core codec (SIMD gated behind feature)

### Threat Model

- Defender against data corruption
- Defender against silent errors
- Not a defense against malicious input (assume trusted source)

---

## Future Improvements

- SIMD acceleration for encode (currently scalar-only)
- COMP-2 codec
- COMP-5 codec
- Parallel batch processing
- WebAssembly build
