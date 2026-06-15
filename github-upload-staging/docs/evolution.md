# Project Evolution

This document records the engineering phases that shaped `cobol_packed` into a
forensic COBOL record tooling platform. It is intentionally phase-based rather
than date-based: the goal is accurate provenance, not fabricated history.

## Phase 1: Codec Core

The project started as a Rust codec for IBM Enterprise COBOL COMP-3 packed
decimal fields. The core library focused on exact byte lengths, sign-nibble
validation, normalized decode/encode, and lossless handling for dirty production
data such as negative zero and non-preferred positive signs.

Key outcomes:

- Stable public encode/decode APIs for packed decimal fields.
- Lossless decode paths that preserve original sign nibbles.
- Optional SIMD nibble expansion checked against scalar reference behavior.
- Property tests, benchmarks, and formal-proof harnesses around codec behavior.

## Phase 2: Codec Hardening

The next pass treated the library as an adversarial input boundary. The work
removed public panic paths, preserved compatibility for legacy truncating encode
behavior, and added strict precision-safe encode APIs for migration code that
must reject fractional digit loss.

Key outcomes:

- Public encode paths reject invalid output buffer sizes instead of panicking.
- SIMD validation rejects oversized input before expensive expansion.
- Strict encode APIs report precision loss explicitly.
- Sign override validation runs even when zero canonicalization later chooses a
preferred zero sign.
- Documentation now states where legacy APIs intentionally truncate toward zero.

## Phase 3: Operational CLI

The CLI became the operational layer around the codec core. The design moved
from single-field helpers toward repeatable migration and audit workflows:
schema validation, streaming batch decode, deterministic machine output, stable
exit codes, and evidence-oriented audit summaries.

Key outcomes:

- `cobol-packed decode`, `encode`, and `inspect` for field workbench use.
- `batch decode`, `batch verify`, and `profile` for fixed-width and structured
batch workflows.
- Stable JSONL/CSV/audit output shapes for automation.
- Schema hashes, input hashes, record counts, anomaly summaries, and bounded
sample failures.
- Redacted full evidence mode for runtime metadata.

## Phase 4: Forensic Record Verification

The forensic release-candidate pass raised verification from field-level checks
to record-level evidence. Layout coverage became explicit: fields, fillers,
SYNC slack, OCCURS ranges, and REDEFINES bases can be accounted for separately.

Key outcomes:

- `--strict-record` verification and `verification_scope` schema support.
- Coverage reports with gaps, overlaps, covered bytes, and full-coverage status.
- Raw malformed record previews for actionable failure samples.
- Description-insensitive semantic schema hashing.
- Explicit JSONL type errors and safer recovery-mode behavior.

## Phase 5: Schema v2 Record Engine

Schema v2 introduced a real COBOL record-layout engine rather than extending the
flat COMP-3 schema indefinitely. The engine separates raw schema declarations
from planned absolute layouts and dispatches through codec-specific field
decoders.

Key outcomes:

- Mixed field types: packed decimal, zoned decimal, binary/native binary, IBM
  hexadecimal float, alphanumeric text, and filler bytes.
- EBCDIC and ASCII overpunch handling for zoned decimal and text workflows.
- Explicit endian handling for binary and float fields.
- Sequential and declared layout modes with SYNC slack planning.
- OCCURS DEPENDING ON with fail-fast counter validation and dynamic record
  length computation.
- Safe REDEFINES decoding through immutable byte windows; no `union`,
  `transmute`, raw pointers, or unsafe layout aliasing.

## Phase 6: Generated Rust And Migration Utilities

The final platform pass added schema-driven Rust generation and migration
supporting commands without pretending to be a complete COBOL compiler.

Key outcomes:

- `schema emit-rust` generates safe typed accessors over `&[u8]`.
- Generated REDEFINES views can evaluate selector fields from the full record.
- Top-level OCCURS groups generate `Vec<Element>` accessors with checked
  counter and record-length validation.
- Unsupported nested generated shapes are rejected instead of emitted partially.
- `schema from-copybook` imports a strict flat copybook subset and rejects hard
  clauses that need compiler-grade layout semantics.
- `schema compare` reports planned layout and codec changes across schemas.

## Phase 7: Release And Verification Discipline

The repository now includes CI, release, fuzz, and formal-proof scaffolding so
the tool can be maintained like a serious migration asset rather than a one-off
developer helper.

Key outcomes:

- Cross-platform CI for formatting, clippy, tests, feature combinations, docs,
  examples, and benchmarks.
- Supply-chain audit job and release artifact smoke tests.
- Deep verification workflow for time-boxed cargo-fuzz campaigns and Kani proof
  harnesses.
- Release workflow for binaries, completions, man page, SBOM metadata,
  checksums, and signed checksum support.

## Remaining Boundary

The current codebase is an advanced record engine, not a full COBOL compiler.
Full copybook parsing, nested generated layout support, advanced compiler
dialect modeling, production-scale fuzz campaigns, and real Kani proof
execution belong in the hosted Linux verification pipeline and follow-up
engineering phases.
