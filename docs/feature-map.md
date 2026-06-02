# Feature Map

This map ties the original COBOL migration risks to the implementation surface
that now handles them. It also records what is intentionally still out of scope
so users do not mistake the tool for a full copybook compiler.

`migration-capability-matrix.json` is the canonical status inventory for COBOL
features. This page is a historical risk map and should not duplicate feature
statuses independently; see `docs/migration-capability-matrix.md` for the
validated summary.

| COBOL / Forensic Risk | Current Support | Primary Surface | Evidence |
| --- | --- | --- | --- |
| COMP-3 packed decimal | Supported | Library APIs, CLI field/batch commands, generated fixed-scalar DISPLAY/MOVE/COMPUTE/SORT-key paths | Unit, property, CLI, VM, converter smoke, fuzz-harness compile, Kani-harness compile |
| Oversized malformed packed input | Supported | Codec length validation before SIMD/scalar expansion | Regression tests and SIMD feature tests |
| Silent precision loss risk | Supported through strict APIs | `to_packed_strict*` APIs; legacy truncating APIs documented | Unit tests and docs |
| Non-preferred sign nibbles | Supported | `pfd` / `nopfd`, canonical/lossless modes | Unit and CLI tests |
| Negative zero | Supported | Lossless decode, profile, audit anomaly reporting | Unit and CLI tests |
| Zoned decimal / overpunch | Supported in Schema v2 | EBCDIC zone nibbles and ASCII overpunch mapping | CLI mixed-codec tests |
| Binary COMP / COMP-4 | Supported in Schema v2 | Signed and unsigned 2/4/8-byte fields with explicit endian | CLI mixed-codec tests |
| COMP-5 native binary | Supported as explicit native-binary schema type | Same integer decoding model with explicit source endian | Schema validation and codec dispatch |
| IBM COMP-1 / COMP-2 floats | Supported in Schema v2 | IBM hexadecimal float32/float64 to `f64` | CLI verification tests |
| EBCDIC alphanumeric text | Supported in Schema v2 | Cp037, Cp500, Cp1140, Cp1148 tables; owned UTF-8 output | CLI text tests |
| SYNC / alignment slack | Supported in sequential planning | Platform profile and synthetic `SyncSlack` ranges | Layout tests and coverage reports |
| REDEFINES | Supported safely | Immutable byte-window decoding of all variants | CLI tests and generated Rust tests |
| Selector-backed REDEFINES | Supported | Runtime selector evaluation and generated `selected()` views | Regression tests |
| OCCURS DEPENDING ON | Supported for binary streaming | Preceding scalar counter, fail-fast range validation, dynamic length | ODO tests and generated Rust tests |
| Record-level audit evidence | Supported | `batch verify --strict-record --coverage-report` | CLI audit tests |
| Schema evolution visibility | Supported | `schema compare` | CLI comparison test |
| Flat copybook bootstrap | Partially supported | `schema from-copybook` strict subset importer | CLI importer test |
| Full copybook compiler semantics | Not yet supported | Follow-up layer | Explicit importer rejection for hard clauses |
| Nested generated Rust groups | Not yet supported | `schema emit-rust` rejects unsupported nested group shapes | Regression test |
| COBOL source normalization | Preview support | `cobol-source`, `cobol2rust convert`; fixed/free plus token-boundary COPY REPLACING pseudo-text pairs | Unit and converter smoke tests |
| COBOL syntax/AST pipeline | Preview support | `cobol-syntax` lossless token tree and typed AST | Parser unit tests |
| Semantic diagnostics and IR | Preview support | `cobol-sema`, `cobol-ir`, `cobol-record`; PIC facts, shared record plan, byte storage layout, scoped REDEFINES metadata, repeated FILLER, condition names, qualified references, subscripts/reference modification metadata, CFG, unresolved symbols | Semantic unit tests |
| Runtime-backed Rust generation | Preview support | `cobol-codegen-rust`, byte-backed `cobol-runtime`, vendored `cobol-record`, `cobol2rust` | Converter smoke tests and generated project compile checks |
| Hostile COBOL blocking | Supported for converter preview | AST/sema/codegen diagnostics plus temporary preflight scanner, codegen invariant validation, and pass-sectioned `migration-report.json` | EXEC SQL, unresolved-symbol, unsupported-section, dynamic-ODO, OCCURS-reference, REDEFINES-reference, subscript/reference-modification, codec-accessor tests |
| Long-running fuzz campaigns | CI-only | `.github/workflows/deep-verification.yml` | Local compile smoke; hosted run required |
| Real Kani proofs | CI/toolchain-only | Kani GitHub Action workflow | Local compile smoke; hosted run required |

## Data Flow Map

```text
Raw bytes / hex / CSV / JSONL
  -> schema load and version dispatch
  -> Schema v1 lowering or Schema v2 planning
  -> planned layout with coverage ranges
  -> codec dispatch per field
  -> decoded rows / audit records / generated Rust accessors

COBOL source / copybooks
  -> source normalization and COPY/COPY REPLACING expansion
  -> lexer and typed AST
  -> semantic symbols and reference IR
  -> shared cobol-record storage plan plus compiler IR
  -> vendored-runtime/record Rust codegen or migration-blocking report
```

## Safety Defaults

- No implicit source endian guessing.
- No implicit codepage guessing.
- OCCURS counters fail when outside `min_occurs..=max_occurs`; they are not
  clamped.
- REDEFINES variants decode from immutable byte windows; no unsafe aliasing.
- Generated Rust rejects unsupported nested layouts instead of producing partial
  accessors.
- Successful converter output must not contain unsupported runtime traps; missed
  unsupported statements and unlowered `SEARCH ALL` / `MOVE CORRESPONDING`
  fallback paths are blocked before Rust is emitted.
- `cobol2rust` lowers Data Division into a shared `cobol-record` plan plus
  byte-backed storage IR and blocks
  Procedure Division paths that would require semantics it cannot yet prove.
- Generated procedure code does not guess subscripted OCCURS access, active
  REDEFINES variants, or packed/binary numeric codecs.
- Machine output is deterministic unless full evidence mode is explicitly
  requested.

## Public Positioning

`cobol_packed` is best described as a packed-decimal codec, Schema v2 COBOL
record engine, and early COBOL-to-Rust converter preview. The converter is a
compiler spine with hard diagnostics, not a full dialect-complete modernization
suite.
