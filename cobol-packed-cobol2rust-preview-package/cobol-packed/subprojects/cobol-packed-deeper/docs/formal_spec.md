Formal specification sketch
1. Domains
Let:
- D ∈ {1..18} be the digit capacity.
- S ∈ {0..D} be the implied scale.
- Sign ∈ {0x0..0xF} be the packed sign nibble.
- B be the byte string of length ⌊D/2⌋ + 1.
- V be the decimal value domain representable by rust_decimal.
2. Packed layout
A valid packed field is a nibble stream n[0..N-1] where:
- If D is even, n[0] = 0x0 is a padding nibble.
- n[pad .. pad + D - 1] are decimal digits 0..9.
- n[pad + D] is the sign nibble.
- B[i] = (n[2i] << 4) | n[2i+1].
3. Numeric interpretation
For digits d_0..d_{D-1} and sign σ:
- m = Σ d_i * 10^{D-1-i}
- x = ±m * 10^{-S} according to σ and sign mode.
4. Laws
Scalar reference law
The scalar decoder is the reference model. The SIMD nibble expander is a validation layer only.
Normalized decode/encode
For all valid bytes b in the chosen sign mode:
- decode_scalar(encode(value)) = truncate_to_scale(value)
- encode(decode_scalar(b)) = canonicalize(b)
Lossless decode/encode
For all valid bytes b in the lossless policy domain:
- encode_lossless(decode_lossless(b)) = b
- zero preserves its original sign nibble when and only when that nibble is accepted by the configured sign policy and the selected zero/sign policy preserves it
- unsigned fields may preserve the full positive sign family (0xA, 0xC, 0xE, 0xF) under lossless policies
Safety laws
- No conversion may panic.
- Magnitude must be rejected when it exceeds 10^D - 1 after scale normalization.
- The canonical encode path may normalize zero; the lossless path may not.
- Decimal::MIN must fail with a typed error, not an unwind.
5. Red-team critique
The implementation is only as strong as its policy separation. The danger zones are:
- conflating canonical zero with forensic zero,
- using 10^D instead of 10^D - 1,
- introducing architecture-specific SIMD paths that diverge from the scalar path,
- allowing any helper to reintroduce abs() on externally supplied decimals.
The test suite should therefore enforce:
- exact byte equality for lossless round trips,
- all sign classes under both sign modes,
- all nibble values in the smallest field sizes,
- cross-checks between SIMD and scalar nibble expansion.
6. Streaming and policy notes
- NibbleIter is the zero-allocation reference stream over packed bytes.
- Canonical encode may normalize zero; forensic/lossless encode must preserve the sign nibble when the configured policy permits it.
- SIMD implementations are validation layers, not alternate truth sources.
