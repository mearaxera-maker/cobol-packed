# Enterprise Readiness

`cobol_packed` and `cobol2rust` are migration tools for controlled, evidence-led
COBOL modernization work. They are not a drop-in COBOL compiler replacement.

## Guarantees

- Packed decimal encode/decode APIs preserve the documented scalar truth model.
- The CLI validates schemas before decoding records and emits stable error
  codes for automation.
- The converter fails closed for unsupported COBOL constructs and writes
  `migration-report.json` rather than emitting misleading Rust.
- Generated Rust projects vendor the first-party runtime crates needed by the
  supported converter subset.
- Oracle tests compare selected observable behavior against an external
  GnuCOBOL executable when `cobc` is installed.

## Non-Guarantees

- No guarantee of dialect-complete COBOL compilation.
- No guarantee of equivalence for VSAM, JCL disposition, indexed files,
  record locking, CICS, DLI, EXEC SQL, national/DBCS edge cases, or
  compiler-specific undefined behavior unless explicitly marked supported in
  `migration-capability-matrix.json`.
- No legal, tax, regulatory, or production cutover advice.
- No trademark clearance for a productized downstream name.

## Diagnostic Interpretation

Treat diagnostics as migration control-plane output:

- `E_UNSUPPORTED_*` means the converter intentionally blocked a source feature.
- `E_CODEGEN_*` means an unsupported or insufficiently lowered IR shape reached
  the code generation guardrail and must not be emitted.
- Layout and reference diagnostics mean byte layout or symbol resolution needs
  source analysis before conversion can be trusted.

Supported capability matrix rows define the stable surface. Experimental rows,
including chaos or ABYSS-style coverage, are engineering evidence only and must
not be sold as migration guarantees.

## Recommended Enterprise Flow

1. Freeze source, copybooks, compiler dialect, and sample datasets.
2. Run schema/layout extraction and review byte layouts with COBOL SMEs.
3. Run converter smoke tests and oracle tests where fixtures exist.
4. Review every blocker in `migration-report.json`.
5. Add oracle fixtures for business-critical behavior before accepting a
   migrated component.
6. Archive SBOM, release checksums, capability matrix, oracle evidence, and
   exact tool versions for audit.

## Third-Party Runtime Boundary

GnuCOBOL is an external oracle executable in the standard workflow. The
converter does not link to `libcob` or bundle GnuCOBOL. If a downstream fork
adds linking to GnuCOBOL runtime libraries, that fork must perform a separate
LGPL compliance review and provide any required notices, source access, and
relinking mechanism.
